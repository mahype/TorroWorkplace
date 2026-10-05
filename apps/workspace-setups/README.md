# Workspace Setups

Workspace Setups ist ein Omarchy-Plugin, das einen einzelnen Hyprland-Workspace als **Workspace-Vorlage** speichert und spaeter auf einem beliebigen Workspace wiederherstellt.

Eine Vorlage beschreibt genau den Workspace, der beim Speichern sichtbar war:

- geoeffnete Anwendungen und Fensteranzahl
- Reihenfolge und Struktur gekachelter Fenster (`dwindle`-Baum) mit Teilungsverhaeltnissen
- Floating-Geometrien relativ zum Monitor
- Vollbild-, Maximierungs- und Pin-Zustaende
- soweit ermittelbar: Projektpfade und Terminal-Arbeitsverzeichnisse
- das im Terminal laufende Programm, sofern es auf der Liste interaktiver Programme steht (z. B. `herdr`, `tmux`, `zellij`, `nvim`, `lazygit`); andere Befehle wie `ssh` oder `sudo` werden nicht gespeichert
- die offenen Tabs von Chromium-, Chrome- und Brave-Fenstern, ausgelesen aus der Sitzungsdatei des Browsers; im Panel pro Fenster bearbeitbar

Beim Laden landet die Vorlage im gerade aktiven Workspace, unabhaengig von dessen Nummer.

## Funktionen

- Omarchy-Bar-Widget mit Verwaltungs-Panel auf Englisch und Deutsch
- aktiven Workspace als Vorlage speichern, aktualisieren, umbenennen und loeschen
- Vorlage im aktiven Workspace laden; ist er nicht leer, fragt das Plugin: **Schliessen & laden**, **Hinzufuegen** oder **Abbrechen**
- pro Fenster waehlbar, wie es zurueckkommt:
  - **Immer neues Fenster** (Terminals, Browser, Dateimanager, Web-Apps)
  - **Offenes Fenster herholen** (Programme, die nur einmal laufen koennen, z. B. Spotify, Slack, Signal)
  - **Automatisch**: starten, und wenn kein neues Fenster erscheint, das laufende Fenster herholen (z. B. VS Code mit bereits offenem Ordner)
- Tastenkuerzel pro Vorlage: `SUPER + F1` bis `SUPER + F10` laedt, `SUPER + SHIFT + F1` bis `F10` speichert den aktiven Workspace auf diese Taste (mit Warnung, wenn dort schon eine Vorlage liegt)
- pro Vorlage: **Beim Start wiederherstellen** auf einem waehlbaren Workspace
- Fortschritt, Zeitlimits, Abbruch und Teilfehler
- atomare, versionierte Vorlagen mit begrenzten Sicherungskopien

Zielplattform:

- Omarchy 4.x mit Hyprland im `dwindle`-Layout
- Omarchy Shell auf Basis von Quickshell

## Installation

```bash
omarchy plugin add https://github.com/mahype/workspace-setups.git --enable
```

Anschliessend erscheint Workspace Setups im rechten Bereich der Omarchy-Bar.

### Tastenkuerzel aktivieren

Omarchy-Plugins koennen keine Tastenkuerzel ueber ihr Manifest anmelden. Das Panel zeigt deshalb den Knopf **Tastenkuerzel aktivieren**. Er haengt einmalig einen kleinen Block an `~/.config/hypr/bindings.lua` an, der die Datei `hypr/bindings.lua` des Plugins einbindet, und laedt Hyprland neu. Wird das Plugin entfernt, ignoriert der Block die fehlende Datei.

Alternativ auf der Kommandozeile:

```bash
./bin/workspace-setups install-bindings
```

Die Tastenkuerzel rufen die Omarchy Shell auf (`omarchy-shell workspace-setups loadSlot 3`), damit Rueckfragen im Panel erscheinen koennen.

Hinweis zur Fn-Taste: Viele Tastaturen senden in der oberen Reihe standardmaessig Medientasten. Dann muss `Fn` gedrueckt oder Fn-Lock aktiviert werden (bei der Logitech MX Keys `Fn + Esc`).

## Bedienung per Kommandozeile

```bash
./bin/workspace-setups inspect --json
./bin/workspace-setups save "Entwicklung"
./bin/workspace-setups update "Entwicklung"
./bin/workspace-setups list --json
./bin/workspace-setups load "Entwicklung"                     # nur in einen leeren Workspace
./bin/workspace-setups load "Entwicklung" --mode add          # zu offenen Fenstern hinzufuegen
./bin/workspace-setups load "Entwicklung" --mode replace      # offene Fenster des Workspace schliessen
./bin/workspace-setups load "Entwicklung" --workspace 4
./bin/workspace-setups save-slot F3 [--force]
./bin/workspace-setups load-slot F3
./bin/workspace-setups set-shortcut "Entwicklung" F3|none [--force]
./bin/workspace-setups set-startup "Entwicklung" on --workspace 2
./bin/workspace-setups set-strategy "Entwicklung" window-2 move
./bin/workspace-setups set-language auto|en|de
./bin/workspace-setups rename "Entwicklung" "Arbeit"
./bin/workspace-setups delete "Arbeit"
```

Fehler werden als JSON auf stderr ausgegeben. Exit-Code `3` bedeutet, dass eine Rueckfrage noetig ist (`code`: `workspace_not_empty`, `slot_taken` oder `exists`).

## Daten

```text
~/.config/workspace-setups/config.json
~/.local/share/workspace-setups/templates/*.json
~/.local/share/workspace-setups/backups/
~/.local/state/workspace-setups/restore.json
~/.local/state/workspace-setups/startup.json
```

Profile aus Version 0.1 (`profiles/`) werden nicht uebernommen und koennen geloescht werden.

Vorlagen enthalten keine vollstaendigen Prozessumgebungen. Unsichere eingebettete Shell-Befehle sowie offensichtliche Zugangsdaten in Startargumenten werden nicht gespeichert.

## Entwicklung

```bash
python3 -m unittest discover -v
omarchy plugin validate .
```

Das Python-Backend verwendet ausschliesslich die Standardbibliothek. Der Hintergrunddienst (`Service.qml`) wird von der Omarchy Shell bei Dateiaenderungen nicht neu geladen; nach Aenderungen daran `omarchy-restart-shell` ausfuehren.

## Dokumentation

- [Produktanforderungen](docs/product-specification.md)
- [Technischer Umsetzungsplan](docs/implementation-plan.md)
- [Projektstand und Entscheidungen](docs/project-status.md)

## Abgrenzung

Workspace Setups ist kein Speicherabbild laufender Prozesse. Es rekonstruiert Fenster und ihre Anordnung und nutzt die Sitzungswiederherstellung der jeweiligen Anwendungen. Browser-Tabs werden als URLs wiederhergestellt, nicht mit Verlauf, Formularinhalten oder dem zuletzt aktiven Tab. Terminalprogramme werden neu gestartet, nicht fortgesetzt; Programme mit eigener Sitzung wie `herdr`, `tmux` oder `zellij` verbinden sich dabei wieder mit ihr. Nicht gespeicherte Dokumente koennen nicht extern wiederhergestellt werden.
