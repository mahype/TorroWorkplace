# Workspace Setups

Workspace Setups ist ein Omarchy-Plugin, das den aktuellen Hyprland-Arbeitsbereich als benanntes Setup speichert und spaeter wiederherstellt.

Ein Setup beschreibt den aeusseren Desktopzustand:

- geoeffnete Anwendungen und Fensteranzahl
- Zuordnung zu Monitoren und Workspaces
- Reihenfolge und Struktur gekachelter Fenster
- Teilungsverhaeltnisse und Floating-Geometrien
- Vollbild-, Floating- und Pin-Zustaende
- soweit ermittelbar: Projektpfade und Terminal-Arbeitsverzeichnisse

Das Plugin stellt ein Icon in der Omarchy-Bar bereit. Darueber lassen sich Setups speichern, aktualisieren, laden, umbenennen und loeschen. Optional kann beim Anmelden ein festes, das zuletzt verwendete oder ein auswaehlbares Setup geladen werden.

## Funktionen

- Omarchy-Bar-Widget mit integriertem Verwaltungs-Panel
- Setups speichern, aktualisieren, umbenennen und loeschen
- Programme und mehrere Fenster derselben Anwendung wiederherstellen
- Workspaces, Monitore, `dwindle`-Baum und Teilungsverhaeltnisse wiederherstellen
- Floating-Positionen, Vollbild, Maximierung und Pin-Zustand wiederherstellen
- Ladearten fuer leere Sitzung, Ergaenzen und bestaetigtes Ersetzen
- Fortschritt, Zeitlimits, Abbruch und Teilfehler
- optionales Laden eines festen oder des zuletzt verwendeten Setups beim Anmelden
- optionale Setup-Auswahl nach der Anmeldung
- Adapter fuer Chromium, Visual Studio Code und `foot`
- atomare, versionierte Profile mit begrenzten Sicherungskopien

Zielplattform:

- Omarchy 4.x
- Hyprland mit `dwindle`-Layout
- gleichbleibende Monitorkonfiguration
- Omarchy Shell auf Basis von Quickshell
- zuerst unterstuetzte Anwendungen: Chromium, Visual Studio Code und `foot`

## Installation

Das Plugin kann direkt aus diesem Repository installiert werden:

```bash
omarchy plugin add https://github.com/mahype/workspace-setups.git --enable
```

Anschliessend erscheint Workspace Setups im rechten Bereich der Omarchy-Bar.

Das Plugin veraendert keine Dateien unter `/usr/share/omarchy/`.

## Bedienung

Die vollstaendige Verwaltung erfolgt ueber das Bar-Icon. Das mitgelieferte Backend kann auch direkt verwendet werden:

```bash
./bin/workspace-setups inspect --json
./bin/workspace-setups save "Entwicklung"
./bin/workspace-setups list --json
./bin/workspace-setups load "Entwicklung" --mode empty
./bin/workspace-setups load "Entwicklung" --mode add
./bin/workspace-setups load "Entwicklung" --mode replace --confirm-replace
./bin/workspace-setups update "Entwicklung"
./bin/workspace-setups rename "Entwicklung" "Arbeit"
./bin/workspace-setups delete "Arbeit"
./bin/workspace-setups set-startup fixed "Entwicklung"
./bin/workspace-setups set-startup last
./bin/workspace-setups set-startup ask
./bin/workspace-setups set-startup none
```

`replace` schliesst normale Anwendungsfenster und muss deshalb immer ausdruecklich bestaetigt werden. Das Bar-Panel zeigt dafuer ebenfalls einen Warnhinweis.

## Daten

```text
~/.config/workspace-setups/config.json
~/.local/share/workspace-setups/profiles/*.json
~/.local/share/workspace-setups/backups/
~/.local/state/workspace-setups/restore.json
```

Profile enthalten keine vollstaendigen Prozessumgebungen. Unsichere eingebettete Shell-Befehle sowie offensichtliche Zugangsdaten in Startargumenten werden nicht gespeichert.

## Entwicklung

```bash
python3 -m unittest discover -v
omarchy plugin validate .
```

Das Python-Backend verwendet ausschliesslich die Standardbibliothek.

## Dokumentation

- [Produktanforderungen](docs/product-specification.md)
- [Technischer Umsetzungsplan](docs/implementation-plan.md)
- [Projektstand und Entscheidungen](docs/project-status.md)

## Abgrenzung

Workspace Setups ist kein Speicherabbild laufender Prozesse. Es rekonstruiert Fenster und Arbeitsflaechen und nutzt die Sitzungswiederherstellung der jeweiligen Anwendungen. Nicht gespeicherte Dokumente oder laufende Terminalprozesse koennen nicht extern wiederhergestellt werden.
