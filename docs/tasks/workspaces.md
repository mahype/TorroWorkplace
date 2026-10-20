# Aufgaben: Workspaces mit Tasten, Laden beim Start und einem Dialog (WS)

Issue #243. Konzept: [core.md](../core.md) (Vorlagen), [settings.md](../settings.md) (Seite Workspaces). Entwurf:
[prototypes/workspaces/entwurf.html](../prototypes/workspaces/entwurf.html) – er ist Maßstab für Seite, Dialog und
Tray. Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`. Wer einen Request anfasst, lädt
`torro-api`; nach jeder Funktion prüft `torro-api-review`. Nach jeder UI-Änderung prüft `torro-design-review`.
Referenz für Abläufe (nicht übersetzen): `apps/workspace-setups` (Tastenplätze, `startup`, `ChoiceDialog`).

**Entschieden mit dem Plan zu #243:**
- Tasten 1 … 0 (zehn Plätze), eine Taste gehört höchstens einer Vorlage. Der Modifikator gilt für alle Vorlagen
  und ist einstellbar; Vorgabe Linux Super + Alt, macOS Ctrl + Option, Windows Ctrl + Shift. Eine Taste lädt wie
  „Load“ im Modus `add`.
- Windows Ctrl + Shift statt Ctrl + Alt (entschieden im Review von PR #259): Windows meldet AltGr als Ctrl + Alt, mit
  Ctrl + Alt + Ziffer wären auf deutscher Tastatur `{ [ ] } ² ³` nicht mehr zu tippen; Win + Ziffer mit Alt, Ctrl oder
  Shift belegt Windows selbst.
- Hyprland: Tasten zur Laufzeit über die Lua-Schnittstelle (`eval hl.bind(…)`, `hl.unbind(…)`), nur belegte
  Plätze, nach jedem Neuladen der Konfiguration erneut. macOS und Windows: Crate `global-hotkey` in der
  Ereignisschleife des Trays.
- Laden beim Start einmal pro Anmeldung, im Modus `add`, auf den Workspace, den der gewählte Bildschirm zeigt.
  Fehlt der Bildschirm: Hauptbildschirm (der bei Position 0,0, sonst der erste) oder auslassen, je Vorlage.
- „Save workspace…“ im Tray speichert sofort und öffnet dann ein eigenes kleines Dialogfenster; „Cancel“ löscht
  die gerade gespeicherte Vorlage wieder. Das Dialogfenster schwebt (Hyprland-Fensterregel).
- Fenster der Steuerzentrale und des Dialogs kommen nie in eine Vorlage.
- Vorlagenformat bleibt Schema 3; die neuen Felder sind optional, alte Dateien laden unverändert.
- Je Eigenschaft ein Befehl (`set_template_shortcut` Write, `set_template_startup` Admin, Name weiter über
  `rename_template`); ein Befehl für alles würde Ausgelassenes löschen.

**Für alle Aufgaben gilt:**
- Nutzertexte englisch mit `tr()`/`trf()`; die deutschen Übersetzungen nennt jede Aufgabe im Bericht, WS.8 trägt
  sie in `crates/torro-core/src/i18n/de.rs` ein.
- Tests ohne Hyprland, ohne Netz, ohne echtes Home (`TORRO_DATA_HOME`, `TORRO_STATE_HOME` auf Temp-Verzeichnisse);
  Fenstermanager über `torro_wm::fake::FakeWm`.
- Tastenplatz im Code: `u8` 0–9 = die Ziffertaste; Reihenfolge in Listen 1 … 9, 0.

```
WS.1 ── WS.2 ─┬─ WS.3 ── WS.4
              ├─ WS.5
              └─ WS.6 ── WS.7 ── WS.8
```

### WS.1 Fenstermanager: Bildschirm fokussieren, Tasten, schwebender Dialog
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-wm/src/{lib,types,fake}.rs`,
  `crates/torro-wm/src/hyprland/{mod,dispatch,events,json}.rs`, `crates/torro-wm/src/aerospace/{mod,cli}.rs`,
  `crates/torro-wm/src/glazewm/mod.rs`
- Neue Methoden in `WindowManager` (mit Weitergabe in `Backend`, `Unsupported` → `WmError::Unsupported`):
  - `focus_monitor(&self, monitor: &str)`: der Bildschirm `Monitor::name` bekommt den Fokus, ohne Fenster zu
    bewegen. Hyprland: Lua-Dispatcher für den Monitor-Fokus (Syntax im Wiki 0.56 nachsehen); AeroSpace
    `focus-monitor <name>`; GlazeWM: Befehl laut deren Doku, sonst `WmError::Unsupported`.
  - `version(&self) -> impl Future<Output = Option<String>>`, Vorgabe `None`; Hyprland aus `j/version`.
  - `bind_keys(&self, binds: &[KeyBind])` und `unbind_keys(&self, keys: &[String])`, Vorgabe
    `WmError::Unsupported`. `KeyBind { keys: String, command: Vec<String> }`, `keys` in Hyprlands Schreibweise
    (`SUPER + ALT + 1`). Hyprland: je Eintrag `eval hl.bind(keys, hl.dsp.exec_cmd(…))` bzw. `eval hl.unbind(keys)`
    (Wiki „Binds“: `hl.unbind` braucht exakt dieselbe Schreibweise); jede Zeichenkette durch `lua_string`, das
    Kommando als eine Shell-sichere Zeile (Argumente einzeln quoten).
  - `float_dialogs(&self, class: &str)`, Vorgabe `Ok(())`; Hyprland: benannte Fensterregel wie `cover_rule`
    (`float`, `center`) für genau diese Klasse.
- `WmEvent::ConfigReloaded`: Hyprland liest `configreloaded>>` aus dem Ereignis-Socket; die anderen Backends senden
  ihn nie. Alle `match` auf `WmEvent` nachziehen (nachsehen in `torro-setups` und `torroworkplace`).
- `FakeWm`: Aufrufe aufzeichnen (`Call`), `focus_monitor` setzt den aktiven Workspace auf den des Monitors.
- **API/MCP:** keine.
- **Abnahme:** Tests der Lua-Ausdrücke (Quoting; eine Kennung mit `'`, `"` und `]]`, Leerzeichen im Pfad), Ereignis-Parser für `configreloaded`,
  `FakeWm`-Test für `focus_monitor`; `cargo build` für macOS- und Windows-Ziele bleibt grün (`cfg`).

### WS.2 Gerüst: Vorlagenfelder, Einstellung, Requests, Katalog, Handler
- [x] erledigt
- **Modell:** Opus · **Braucht:** WS.1 · **Dateien:** `crates/torro-setups/src/{template,capture,storage}.rs`,
  `crates/torro-core/src/{settings,app}.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torroworkplace/src/{core,mcp,audit,approvals,apps,daemon,cli,notify}.rs`,
  `crates/torroworkplace/src/tray/{menu,linux}.rs` und `crates/torroworkplace/src/ui/{launch,workspaces}.rs` (nur
  nachziehen, wo `TemplateSummary` gebaut wird), `docs/api.md`, `docs/api-reference.md`
- `Template`: `shortcut: Option<u8>` und `startup: Option<Startup>`, beide `#[serde(default)]`;
  `Startup { screen: String, fallback: ScreenFallback }`, `ScreenFallback = Main | Skip` (Vorgabe `Main`).
  `TemplateSummary` zusätzlich `shortcut`, `startup`, `workspace` und `monitor` (aus `source`).
- `capture`: Fenster mit den Klassen `AppId::WORKPLACE.window_class` und der Dialogklasse
  `torro-workplace-dialog` (neue Konstante neben `AppId` in `torro-core`) werden ausgelassen.
- `Settings.workspaces: WorkspaceSettings { modifiers: ShortcutModifiers }`, `#[serde(default)]`;
  `ShortcutModifiers = SuperAlt | SuperCtrl | CtrlAlt | CtrlShift`, Vorgabe je System wie oben. Neuer
  `SettingKey::WorkspaceShortcuts` (`workspace_shortcuts`); Doc-Kommentar in `SettingKey` und Beschreibung von
  `settings.set_setting` im Katalog nennen Schlüssel und Werte.
- Requests (Bereich `workspaces`), klein und gezielt – jeder ändert genau eine Eigenschaft, Ausgelassenes bleibt:
  - `Request::SetTemplateShortcut { id, key: Option<u8> }` · `workspaces.set_template_shortcut` · Write · MCP
    `workspaces_set_template_shortcut` – `None` nimmt die Taste weg. Hält eine andere Vorlage die Taste, verliert
    sie sie (beide Dateien in einer Schreibrunde unter `exclusive`, atomar). `not-found`, `key` > 9 `error`.
  - `Request::SetTemplateStartup { id, startup: Option<Startup> }` · `workspaces.set_template_startup` · **Admin**
    (eine bleibende Regel, die bei jeder Anmeldung Programme startet, wie `start_at_login`) · MCP
    `workspaces_set_template_startup` – `None` schaltet das Laden beim Start aus. `not-found`, unbekannter
    Bildschirm ist erlaubt (er kann später angeschlossen sein).
  - `Request::ListScreens` · `workspaces.list_screens` · Read · MCP `workspaces_list_screens` –
    `Response::Screens { screens: Vec<ScreenInfo { name, width, height, scale, main: bool }> }`;
    `unsupported` ohne Fenstermanager.
  - Name weiter über `workspaces.rename_template`. `workspaces.save_template` bleibt unverändert; Taste und Start
    setzt der Dialog danach mit den Befehlen oben.
  - Katalogtexte nennen die Nebenwirkungen: Umziehen der Taste, Laden bei jeder Anmeldung.
- `apps.open_app` bekommt das Ziel `template:<id>` (`apps::Target::Template`, `FORMS`, `Target::app`/`opens_in`
  = `AppId::WORKPLACE`, `Opened::Template`, `target_command` → `ui --template=<id>`): öffnet immer den Dialog im
  Modus `Edit`, nie den Modus `Save`. `open_target` verlangt bei `Reach::Client` Leserecht auf `workspaces`
  (sonst `not-found`, wie bei Mail und Aufgaben) und gibt den Namen der Vorlage als Titel zurück. Doc-Kommentar von
  `Request::OpenApp` und Katalogtext von `apps.open_app` nachziehen.
- `Status` (`camelCase`): `wmVersion: Option<String>` und `shortcuts: Option<ShortcutStatus { modifiers, active:
  bool, problem: Option<String> }>`, beide `#[serde(default, skip_serializing_if…)]`; `status()` blendet `shortcuts`
  für Clients ohne Leserecht auf `workspaces` aus wie `workspace` und `templates`. WS.3/WS.4 füllen `shortcuts`.
  Beschreibung von `system.status` ergänzen.
- Freigabe und Protokoll: `facts()` liefert für `SetTemplateShortcut` den Namen der Vorlage und über
  `Facts::moved_from` den der bisherigen Inhaberin, für `SetTemplateStartup` den Namen; `approvals.rs` fasst
  zusammen („Give "{name}" key {key}“, mit „, taken from "{other}"“; „Load "{name}" at every login on {screen}“ bzw.
  „… no longer at login“); `audit.rs` hasht die Kennung und bei `SetTemplateShortcut` auch die der bisherigen
  Inhaberin; `daemon::describe` und `daemon::changes_state` kennen die neuen Befehle.
- `ctl setups` bekommt nur `key <id> <0-9|none>`; Start und Bildschirme bleiben Oberfläche, API und MCP
  (Startregeln von der Kommandozeile braucht niemand, der den Dienst nicht schon per API anspricht).
- `docs/api.md`: neue Zeilen, Fußnote zu `set_template_startup` (Admin) und zum Ziel `template:<id>`.
- **Oberfläche:** WS.6 · **API/MCP:** wie oben.
- **Abnahme:** Round-Trip einer alten Vorlage ohne neue Felder; Handler-Tests für Umziehen der Taste, Taste
  wegnehmen, unbekannte Kennung, `list_screens` mit `FakeWm`; Rechte-Tabelle grün plus eigener Fall
  `template:<id>` ohne Leserecht → `not-found`; MCP-Test, dass `workspaces_set_template_shortcut` mit `{id, key:
  null}` die Taste wegnimmt; `TORRO_BLESS=1 cargo test -p torro-ipc` erzeugt `docs/api-reference.md` neu.

### WS.3 Tasten unter Hyprland
- [x] erledigt
- **Modell:** Opus · **Braucht:** WS.2 · **Dateien:** `crates/torroworkplace/src/workspace_keys.rs` (neu),
  `crates/torroworkplace/src/{core,main}.rs`
- `workspace_keys`: aus Vorlagen und `Settings.workspaces.modifiers` die Liste `KeyBind` bauen (Kommando
  `<eigenes Binary> ctl setups load <id> --mode add`, Binary wie `core::sibling`), Unterschied zur zuletzt
  angemeldeten Liste anwenden (`unbind_keys` für Weggefallenes, `bind_keys` für Neues).
- Anwenden beim Start des Dienstes, nach jedem Speichern/Bearbeiten/Löschen einer Vorlage, nach Änderung des
  Modifikators und bei `WmEvent::ConfigReloaded` (dann alles neu, die alte Liste gilt als leer).
- `Status.shortcuts` füllen: `active`, bei Fehlern von Hyprland `problem` mit der Taste („Super + Alt + 5 could not
  be registered“).
- Ohne Hyprland tut das Modul nichts (macOS/Windows: WS.4).
- **API/MCP:** keine neuen; `system.status` zeigt den Zustand.
- **Abnahme:** Tests mit `FakeWm`: Diff beim Umziehen einer Taste, Neuanmeldung nach `ConfigReloaded`,
  Modifikatorwechsel meldet alle alten Tasten ab.

### WS.4 Tasten unter macOS und Windows
- [x] erledigt
- **Modell:** Opus · **Braucht:** WS.3 · **Dateien:** `Cargo.toml`, `crates/torroworkplace/Cargo.toml`,
  `crates/torroworkplace/src/{workspace_keys,daemon}.rs`, `crates/torroworkplace/src/tray/desktop.rs`
- Abhängigkeit `global-hotkey` in `[workspace.dependencies]` (Version, die mit `tray-icon` 0.26 und `winit` 0.30
  zusammenpasst, mindestens zwei Wochen alt), im Crate nur unter `cfg(any(target_os = "macos", windows))`.
- Der `GlobalHotKeyManager` lebt auf dem Hauptthread in der winit-Schleife (`Shell`); die Belegung kommt mit dem
  `Snapshot` (Taste → Vorlagen-Id und Modifikator), ein Tastendruck läuft wie ein Menüklick als
  `Daemon::run(Request::LoadTemplate { id, mode: Add })`. Fehler beim Anmelden (vom System belegt) meldet die
  Schleife an den Kern zurück, `Status.shortcuts.problem` zeigt sie.
- Modifikator-Abbildung: Super = Cmd (macOS) bzw. Win; Option = Alt.
- **Abnahme:** Unit-Test der Abbildung Platz/Modifikator → `HotKey`; Build für macOS (lokal); Windows mit
  `cargo check --target x86_64-pc-windows-msvc -p torroworkplace`, wenn das Ziel installiert ist, sonst im Bericht
  als nicht geprüft nennen (`scripts/ci-local.sh` prüft Windows nicht). Echte Tasten: von Hand.

### WS.5 Laden beim Start
- [x] erledigt
- **Modell:** Opus · **Braucht:** WS.2 · **Dateien:** `crates/torroworkplace/src/startup_templates.rs` (neu),
  `crates/torroworkplace/src/{core,main}.rs`
- Beim Start des Dienstes, im Hintergrund: Merker `startup.json` im State-Verzeichnis mit Sitzungskennung
  (`HYPRLAND_INSTANCE_SIGNATURE`, sonst Systemstart aus `sysinfo::System::boot_time()`); gleich → nichts tun.
- Sonst warten, bis `monitors()` dreimal in Folge gleich antwortet (Abstand 1 s, höchstens 60 Versuche), dann
  jede Vorlage mit `startup` der Reihe nach (Taste, dann Name): Bildschirm suchen, sonst nach `fallback`
  Hauptbildschirm oder auslassen; `focus_monitor`, dann laden wie `LoadTemplate { mode: Add }` unter
  `exclusive`. Danach den Bildschirm fokussieren, der vorher den Fokus hatte; Merker schreiben.
- Fehler einer Vorlage stoppen die anderen nicht; Bericht wie `daemon::report`.
- **API/MCP:** keine; „beim Start“ ist eine Eigenschaft der Vorlage (WS.2).
- **Abnahme:** Tests mit `FakeWm` und Temp-State: zweiter Start in derselben Sitzung lädt nichts, fehlender
  Bildschirm mit `Main`/`Skip`, Reihenfolge, Fokus zurück.

### WS.6 Oberfläche: Seite Workspaces und Dialog
- [x] erledigt
- **Modell:** Opus · **Braucht:** WS.2 · **Dateien:** `crates/torroworkplace/src/ui/{workspaces,template_form,mod,launch}.rs`
  (`template_form.rs` neu), `crates/torroworkplace/src/main.rs`
- `template_form`: der Dialog aus dem Entwurf (Name, Tastenreihe „None“ + 1 … 0 mit Name der Inhaberin, Hinweis
  beim Umziehen, Abschnitt „Startup“ mit Schalter, Bildschirmskizze in echten Proportionen aus
  `workspaces.list_screens`, „If the screen is missing“); Modus `Edit` und `Save` (Titel „Save workspace“, Zeile
  mit den erfassten Programmen, „Cancel“ schickt `DeleteTemplate`). Speichern schickt nur, was sich geändert hat:
  `RenameTemplate`, `SetTemplateShortcut`, `SetTemplateStartup`, in dieser Reihenfolge; ein Fehler bleibt im Dialog stehen. In Schmal
  zwei Reihen Tasten, Bildschirme als Chips.
- Seite Workspaces: Gruppe „Window manager“ (Name, Version aus `Status.wm_version`, Bildschirme; Zeile „Shortcuts“
  mit Auswahl des Modifikators über `settings.set_setting` und dem Zustand aus `Status.shortcuts`), Vorlagen
  sortiert nach Taste, Zeile mit `kbd`, Pille „At startup · DP-1“, „Load“ und „…“ (Edit…, Update from current
  workspace, Delete…); Primärknopf „Save workspace…“ (schickt `SaveTemplate`, öffnet dann den Dialog im Modus
  `Save`). Umbenennen in der Zeile entfällt. Ohne Fenstermanager: Zeile mit erwartetem Namen und was zu tun ist.
- `Data.screens` über `workspaces.list_screens` laden.
- Eigenes Dialogfenster: `torroworkplace ui --template=<id> [--saved]` öffnet nur den Dialog (Klasse
  `torro-workplace-dialog`, Größe `size::form_max()` breit), schließt nach Speichern oder Abbrechen. `--saved`
  (Modus `Save`) setzen nur der Tray (WS.7) und die Seite selbst, nie das Ziel `template:<id>` aus WS.2.
- Demo-Zustände für `--demo --open`: `edit-template` (Taste belegt), `save-template`, `workspaces-unsupported`.
- **API/MCP:** keine neuen; das Ziel `template:<id>` entsteht in WS.2.
- **Abnahme:** Update-Tests der Seite und des Formulars (Umziehen, Cancel im Modus `Save` löscht, Escape),
  `Launch::parse` für die neuen Parameter, Sichtprüfung aller Größen mit `torro-design-review`.

### WS.7 Tray-Menü
- [x] erledigt
- **Modell:** Opus · **Braucht:** WS.6 · **Dateien:** `crates/torroworkplace/src/tray/{menu,desktop,linux}.rs`,
  `crates/torroworkplace/src/{daemon,core}.rs`
- `templates_section`: Info-Zeile „Workspaces · Hyprland“, je Vorlage ein Untermenü in der ersten Ebene, Name mit
  Taste dahinter („Deep work · 1“; Tray-Menüs kennen keine rechtsbündigen Hinweise), Einträge „Load here“, „Add“,
  „Replace“, „Edit…“ (öffnet den Dialog). Danach „Save workspace…“ und „Manage workspaces…“ (Steuerzentrale auf
  `--section workspaces`, wie `show_control_centre`). „Update“ und „Delete“ entfallen im Tray.
- Neue `Action::SaveWorkspace`: `SaveTemplate` ohne Namen, bei `Saved` den Dialog mit `--saved` starten.
- Beim Start des Dienstes `float_dialogs("torro-workplace-dialog")`.
- **API/MCP:** keine neuen. „Manage workspaces…“ hat keinen Request: Es ist reine Navigation in die eigene
  Steuerzentrale (wie „Settings…“), ändert nichts und hat außerhalb der Oberfläche kein Ziel.
- **Abnahme:** Tests von `build()` für Vorlagen mit/ohne Taste, ohne Fenstermanager; Test, dass `SaveWorkspace` den
  Dialog mit der neuen Kennung startet (Launcher-Fake).

### WS.8 Übersetzungen und Doku
- [x] erledigt
- **Modell:** Haiku · **Braucht:** WS.7 · **Dateien:** `crates/torro-core/src/i18n/de.rs`, `docs/core.md`,
  `docs/settings.md`
- Alle neuen Texte aus WS.2–WS.7 übersetzen. `core.md` („Funktionen“, „Tray-Menü“, „Vorlagenformat“) und
  `settings.md` (Seite Workspaces) auf Tasten 1 … 0, Start und Dialog bringen; F1–F10 als verworfen nennen.
- **Abnahme:** `TORRO_LANG=de cargo test --workspace` grün.
