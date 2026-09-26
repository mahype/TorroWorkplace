# Technischer Umsetzungsplan

Stand: Version 0.2 (Workspace-Vorlagen)

## Architektur

```text
BarWidget.qml ── Panel.qml ── ChoiceDialog.qml
      │              │
      └──── Service.qml (IPC "workspace-setups", Startdienst, Rueckfragen)
                     │  Process
                     ▼
            bin/workspace-setups (Python, nur Standardbibliothek)
              cli.py      Befehle, JSON-Ausgabe, Exit-Codes
              capture.py  aktiven Workspace erfassen
              restore.py  Vorlage in Ziel-Workspace aufbauen
              storage.py  Schema-Validierung, Vorlagen, Tastenplaetze
              commands.py Startbefehle, Adapter, Standardstrategien
              bindings.py Einbindung der Tastenkuerzel in Hyprland
              layout.py   Kachelbaum aus Fensterrechtecken
              hyprland.py hyprctl-Abfragen und Lua-Dispatcher
```

`I18n.js` enthaelt alle Oberflaechentexte auf Englisch und Deutsch; `Model.js` normalisiert die Backend-Ausgaben.

## Omarchy-Integration

- `Service.qml` wird mit der Omarchy Shell geladen und stellt das IPC-Ziel `workspace-setups` mit `loadSlot`, `saveSlot` und `refresh` bereit.
- Braucht das Backend eine Entscheidung, setzt der Service `pendingChoice` und oeffnet das Panel ueber `shell.summon`.
- Die Shell laedt `Service.qml` bei Dateiaenderungen nicht neu; dafuer ist `omarchy-restart-shell` noetig.
- Tastenkuerzel: `hypr/bindings.lua` bindet `SUPER + F<n>` und `SUPER + SHIFT + F<n>` an `omarchy-shell -q workspace-setups loadSlot|saveSlot <n>`. `install-bindings` haengt einen durch einen Marker erkennbaren Block an `~/.config/hypr/bindings.lua` an, der diese Datei nur einbindet, wenn sie existiert, und ruft `hyprctl reload` auf. Die Belegung der Plaetze liegt in den Vorlagen, daher muss die Hyprland-Konfiguration bei Aenderungen nicht neu erzeugt werden.

## Backend-Schnittstelle

| Befehl | Zweck |
|---|---|
| `list --json` | Vorlagen, Sprache, Status der Tastenkuerzel |
| `inspect --json` | aktiven Workspace erfassen, ohne zu speichern |
| `save <name>` / `update <name>` | aktiven Workspace speichern |
| `save-slot <F1..F10> [--force]` | speichern auf Tastenplatz |
| `load <name>` / `load-slot <F..>` | `--mode ask\|add\|replace`, `--workspace` |
| `set-shortcut`, `set-startup`, `set-strategy`, `set-language` | Einstellungen |
| `startup [--force]` | Start-Vorlagen einmal pro Hyprland-Sitzung laden |
| `bindings-status`, `install-bindings` | Tastenkuerzel |
| `status --json`, `cancel` | Fortschritt und Abbruch |

Fehler erscheinen als JSON auf stderr. Fehler mit `code` `workspace_not_empty`, `slot_taken` oder `exists` enden mit Exit-Code 3 und werden im Panel als Rueckfrage angeboten. Der Service wiederholt danach den Befehl mit `--mode replace|add` beziehungsweise `--force`.

## Datenspeicherung

```text
~/.config/workspace-setups/config.json           Sprache, Zeitlimit
~/.local/share/workspace-setups/templates/*.json  Vorlagen
~/.local/share/workspace-setups/backups/          letzte fuenf Staende je Vorlage
~/.local/state/workspace-setups/restore.json      Fortschritt
~/.local/state/workspace-setups/restore.lock      Restore-Lock
~/.local/state/workspace-setups/startup.json      Hyprland-Instanz des letzten Startlaufs
```

## Vorlagenformat (Schema 2)

```json
{
  "schemaVersion": 2,
  "id": "entwicklung",
  "name": "Entwicklung",
  "source": {"workspace": "3", "monitor": "DP-1", "monitorSize": {"width": 7680, "height": 2160, "scale": 2}},
  "shortcut": 3,
  "restoreOnStartup": true,
  "startupWorkspace": "3",
  "tree": {"split": "vertical", "ratio": 0.5, "first": {"window": "window-1"}, "second": {"window": "window-2"}},
  "windows": [{
    "id": "window-1", "application": "chromium", "command": ["/usr/bin/chromium"],
    "initialClass": "chromium", "class": "chromium", "titleHint": "…",
    "strategy": "new", "floating": false, "fullscreen": false, "fullscreenMode": "fullscreen", "pinned": false,
    "geometry": {"x": 12, "y": 32, "width": 1901, "height": 1036}
  }]
}
```

`geometry` ist relativ zum Ursprung des Monitors. Der Baum muss jedes gekachelte Fenster genau einmal enthalten; `shortcut` ist eindeutig ueber alle Vorlagen.

## Erfassung

- `activeworkspace` bestimmt Workspace und Monitor; nur dessen normale Fenster werden erfasst. Portal- und Polkit-Dialoge werden ignoriert.
- Startbefehle stammen bevorzugt aus Desktop-Eintraegen, sonst aus `/proc/<pid>/cmdline`, bereinigt um Platzhalter, interne Flags und moegliche Geheimnisse.
- `default_strategy` waehlt `new` fuer Terminals, Browser, Dateimanager und Web-Apps, `move` fuer bekannte Einzelinstanz-Programme und sonst `auto`.
- Beim Aktualisieren bleiben Tastenplatz, Start-Einstellungen und je Fenstertitel gewaehlte Strategien erhalten.

## Wiederherstellung

1. Ziel-Workspace bestimmen (aktiv oder `--workspace`) und festhalten.
2. Im Modus `ask` bei vorhandenen Fenstern mit `workspace_not_empty` abbrechen, bevor etwas veraendert wird.
3. Restore-Lock setzen; bei `replace` nur die Fenster des Ziel-Workspace schliessen und auf ihr Verschwinden warten.
4. Fenster in Baumreihenfolge beschaffen: je nach Strategie herholen, starten oder starten mit Rueckfall aufs Herholen nach hoechstens 3 Sekunden. Neu gestartete Fenster werden nur unter Fenstern gesucht, die vor dem Laden nicht existierten.
5. Floating-Fenster sofort in den Ziel-Workspace verschieben und relativ zum Zielmonitor platzieren.
6. Gekachelte Fenster auf einen temporaeren Staging-Workspace legen und in Baumreihenfolge mit `preselect` und `splitratio` in den Ziel-Workspace einfuegen. Bei Fehlern werden gestagte Fenster in den Ziel-Workspace zurueckgelegt.
7. Nur bei gleicher Monitorgroesse feste Groessen fuer gekachelte Fenster setzen; danach Pin und Vollbild anwenden.
8. Ziel-Workspace fokussieren (beim Startlauf am Ende den urspruenglich aktiven).

## Sicherheitsanforderungen

- Nie ungefragt Fenster beenden; `replace` betrifft nur den Ziel-Workspace und wird im Panel bestaetigt.
- Keine Shell-Interpolation fuer Namen oder Startargumente; Lua-Dispatcher erhalten ausschliesslich escapte Zeichenketten und normalisierte Fensteradressen.
- Keine Secrets oder Prozessumgebungen speichern; eingebettete Shell- und Interpreter-Befehle werden abgelehnt.
- Vorlagendateien atomar schreiben und symbolische Links ablehnen.
- Die Aenderung an `~/.config/hypr/bindings.lua` erfolgt nur auf ausdruecklichen Klick und ist idempotent.

## Tests

`python3 -m unittest discover` deckt ab: Validierung und Tastenplaetze, Erfassung nur des aktiven Workspace, relative Geometrie, Standardstrategien, alle drei Strategien einschliesslich Rueckfall, die Modi `ask`/`add`/`replace`, benannte und explizite Ziel-Workspaces, Kachelbaum-Plan, Abbruch, Fehlerpfade beim Staging, CLI-Exit-Codes, einmaligen Startlauf und die Einbindung der Tastenkuerzel.

Manuelle Abnahme: Workspace mit mehreren gekachelten Fenstern speichern, auf einem leeren Workspace laden, auf einem belegten Workspace die Rueckfrage pruefen, ein Einzelinstanz-Programm von einem anderen Workspace herholen, Tastenkuerzel und Startlauf nach einer frischen Anmeldung pruefen.
