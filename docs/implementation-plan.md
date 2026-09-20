# Technischer Umsetzungsplan

Status: Die beschriebenen Kernphasen sind implementiert. Optionale Vorschaubilder, Browser-Tab-Erfassung und Profil-Synchronisation bleiben Erweiterungen fuer spaetere Versionen.

## Architektur

Das Projekt wird als ein Omarchy-Plugin ausgeliefert, intern jedoch in Oberflaeche und Backend getrennt.

```text
workspace-setups/
├── manifest.json
├── BarWidget.qml
├── Panel.qml
├── Service.qml
├── Model.js
├── bin/
│   └── workspace-setups
├── tests/
└── docs/
```

Das Manifest deklariert mindestens ein `bar-widget` und einen `service`. Der Service bleibt waehrend der Omarchy-Shell-Sitzung geladen und steuert das Verhalten nach der Anmeldung. Das Widget oeffnet ein in das Plugin integriertes Panel.

Die QML-Seite bleibt fuer Darstellung, Eingaben und Prozessfortschritt verantwortlich. Ein Python-Backend ohne externe Bibliotheken uebernimmt Dateizugriffe, Prozessanalyse und Hyprland-IPC. Diese Trennung haelt komplexe Wiederherstellungslogik testbar und verhindert, dass langfristige Prozesse an den Lebenszyklus eines Panels gekoppelt werden.

## Omarchy-Integration

Stabile Plugin-ID:

```text
io.github.mahype.workspace-setups
```

Installationspfad:

```text
~/.config/omarchy/plugins/io.github.mahype.workspace-setups/
```

Das Plugin veraendert keine Dateien unter `/usr/share/omarchy/`. Es wird ueber `omarchy plugin validate`, `omarchy plugin enable` und die bestehenden Bar-Befehle integriert.

## Backend-Schnittstelle

Die erste CLI-Schnittstelle:

```bash
workspace-setups list --json
workspace-setups inspect --json
workspace-setups save NAME
workspace-setups update NAME
workspace-setups load NAME --mode empty
workspace-setups delete NAME
workspace-setups rename OLD NEW
workspace-setups set-startup MODE [NAME]
workspace-setups status --json
workspace-setups cancel
```

QML startet Befehle ueber Quickshell `Process` mit getrennten Argumenten und wertet JSON-Ausgaben aus. Es werden keine aus Benutzereingaben zusammengesetzten Shell-Befehle ausgefuehrt.

Laenger laufende Ladeprozesse schreiben strukturierte Fortschrittsereignisse, beispielsweise als JSON Lines. Die Oberflaeche kann dadurch einzelne Fenster als wartend, erfolgreich oder fehlgeschlagen darstellen.

## Datenspeicherung

```text
~/.config/workspace-setups/config.json
~/.local/share/workspace-setups/profiles/<slug>.json
~/.local/state/workspace-setups/restore.json
~/.local/state/workspace-setups/workspace-setups.log
```

- `config.json` enthaelt Startmodus, Standardprofil und Zeitlimits.
- `profiles/` enthaelt nutzergenerierte Setups.
- `restore.json` beschreibt nur einen aktiven oder zuletzt abgeschlossenen Ladevorgang.
- Das Log enthaelt technische Diagnoseinformationen ohne sensible Umgebungsvariablen.

Alle Profildateien erhalten ein `schemaVersion`-Feld. Schreibvorgaenge erfolgen atomar ueber eine temporaere Datei und anschliessendes Umbenennen. Vor dem Aktualisieren wird eine begrenzte Sicherung der vorherigen Version angelegt.

## Vorlaeufiges Profilformat

```json
{
  "schemaVersion": 1,
  "id": "entwicklung",
  "name": "Entwicklung",
  "createdAt": "2026-09-19T12:00:00Z",
  "updatedAt": "2026-09-19T12:00:00Z",
  "environment": {
    "hyprlandLayout": "dwindle",
    "monitors": [
      {
        "name": "DP-1",
        "width": 3840,
        "height": 1080,
        "scale": 1.0,
        "x": 0,
        "y": 0
      }
    ]
  },
  "workspaces": [
    {
      "id": 1,
      "name": "1",
      "monitor": "DP-1",
      "layout": "dwindle",
      "tree": {}
    }
  ],
  "windows": [
    {
      "id": "window-1",
      "application": "foot",
      "command": ["foot", "--working-directory", "/home/user/Projects"],
      "initialClass": "foot",
      "titleHint": "Projects",
      "workspace": "1",
      "monitor": "DP-1",
      "floating": false,
      "fullscreen": false,
      "pinned": false,
      "geometry": {
        "x": 12,
        "y": 12,
        "width": 701,
        "height": 611
      }
    }
  ]
}
```

Das Format speichert keine vollstaendigen Umgebungsvariablen, Zugriffstoken oder Session-Cookies.

## Erfassung

### Hyprland-Zustand

Das Backend liest mindestens:

```bash
hyprctl -j monitors
hyprctl -j workspaces
hyprctl -j clients
hyprctl -j activeworkspace
```

Layer-Shell-Oberflaechen wie die Omarchy-Bar sind keine normalen Clients und werden nicht als Anwendungsfenster gespeichert. Zusaetzliche Ausschlussregeln koennen Plugin-eigene Fenster, Launcher und temporaere Dialoge filtern.

### Startbefehle

Die generische Aufloesung verwendet:

1. PID und Prozesshierarchie des Fensters,
2. `/proc/<pid>/cmdline`,
3. passende Desktop-Datei anhand von `StartupWMClass` oder Programmname,
4. bekannte sichere Startargumente,
5. programmspezifischen Adapter als bevorzugte Quelle.

Argumente mit Tokens, temporaeren Dateien oder internen Prozessschaltern muessen herausgefiltert werden. Kann kein sicherer Startbefehl bestimmt werden, wird das Fenster im Speicherdialog markiert.

### Kachelbaum

`hyprctl clients` liefert Fenstergeometrien, aber keinen vollstaendigen serialisierten `dwindle`-Baum. Der Baum wird deshalb aus den Rechtecken eines Workspaces rekonstruiert:

1. Gesamtflaeche des Workspaces bestimmen.
2. Nach einer vollstaendigen horizontalen oder vertikalen Trennlinie suchen.
3. Fenster in zwei Teilmengen aufteilen.
4. Vorgang rekursiv fortsetzen.
5. Trennrichtung und relatives Teilungsverhaeltnis speichern.

Kann kein eindeutiger Baum bestimmt werden, bleibt die Geometrie als Fallback erhalten und die Oberflaeche zeigt eine Warnung.

## Wiederherstellung

### Vorpruefung

- Profilversion validieren.
- benoetigte Programme aufloesen,
- aktuelle Monitore mit dem gespeicherten Fingerabdruck vergleichen,
- bestehenden Ladeprozess ausschliessen,
- Ladeoption fuer vorhandene Fenster anwenden.

### Programme starten

Programme werden ueber die UWSM-Sitzung gestartet. Das Backend wartet parallel auf den Hyprland-Event-Socket und verarbeitet neue Fensterereignisse, statt mit festen Pausen zu arbeiten.

Jedes erwartete Fenster erhaelt Matching-Merkmale:

- PID oder Nachfahre des gestarteten Prozesses,
- `initialClass`,
- aktuelle Klasse,
- Titelhinweis,
- erwartete Reihenfolge innerhalb derselben Anwendung,
- erwarteter Workspace.

Die Kombination ist notwendig, weil Chromium und andere Anwendungen mehrere Fenster in einem gemeinsamen Prozess oeffnen koennen.

### Fenster platzieren

Nach erfolgreichem Matching:

1. Fenster auf gespeicherten Workspace und Monitor verschieben.
2. Floating- oder Tiling-Zustand herstellen.
3. Gekachelte Fenster in Baumreihenfolge einfuegen.
4. Split-Richtung und Teilungsverhaeltnisse anwenden.
5. Floating-Fenster auf absolute Position und Groesse setzen.
6. Vollbild- und Pin-Zustand zuletzt anwenden.

Alle Hyprland-Aktionen adressieren nach Moeglichkeit die konkrete Fensteradresse, nicht nur die Fensterklasse.

### Fehlerbehandlung

- Jedes Fenster besitzt ein eigenes Zeitlimit.
- Ein Fehler stoppt nicht die restliche Wiederherstellung.
- Ein Abbruch verhindert weitere Starts, schliesst aber bereits gestartete Fenster nicht automatisch.
- Das Endergebnis unterscheidet erfolgreich, fehlgeschlagen, uebersprungen und manuell abgebrochen.

## Anwendungsadapter

### Generisch

Startet eine Anwendung ueber einen bereinigten Desktop-Eintrag oder einen gespeicherten Argumentvektor. Der interne Anwendungszustand bleibt der Anwendung ueberlassen.

### Chromium

Erste Version:

- Chromium einmal starten,
- eigene Sitzungswiederherstellung verwenden,
- erscheinende Browserfenster den gespeicherten Fenstern zuordnen,
- Fenster auf Workspaces verteilen.

Spaeter optional: Browser-Erweiterung zur expliziten Erfassung von Fenster- und Tab-URLs.

### Visual Studio Code

- gespeicherte Ordner- oder Workspace-Datei bevorzugt per CLI oeffnen,
- andernfalls die VS-Code-Sitzungswiederherstellung verwenden,
- mehrere Fenster einzeln zuordnen.

### foot

- Arbeitsverzeichnis des aktiven Shell-Kindprozesses ermitteln,
- neues Fenster mit diesem Arbeitsverzeichnis starten,
- keine laufenden Shell-Prozesse oder Terminalpuffer versprechen,
- optional spaeter `tmux`- und `zellij`-Sitzungen erkennen.

## Startdienst

`Service.qml` wird zusammen mit der Omarchy-Shell geladen. Abhaengig von der Konfiguration:

- beendet er sich logisch ohne Aktion,
- wartet auf Hyprland und laedt das Standardprofil,
- laedt das zuletzt verwendete Profil,
- oder oeffnet nach kurzer Bereitschaftspruefung die Profilauswahl.

Der Dienst muss einen Restore-Lock verwenden, damit Hot-Reload oder ein zweiter Klick keinen parallelen Ladevorgang startet.

## Sicherheitsanforderungen

- Nie ungefragt Fenster oder Prozesse beenden.
- Der Modus `replace` benoetigt eine ausdrueckliche Bestaetigung.
- Keine Shell-Interpolation fuer Profilnamen oder Startargumente.
- Keine Secrets oder vollstaendigen Prozessumgebungen speichern.
- Profildateien vor Pfadmanipulation und symbolischen Link-Angriffen schuetzen.
- Ein Profil darf nur bekannte schema-validierte Felder ausfuehren.
- Technische Logs muessen Startargumente mit moeglichen Geheimnissen maskieren.

## Tests

### Unit-Tests

- Profilvalidierung und Migration,
- atomare Speicherung,
- Rekonstruktion des Kachelbaums aus Fensterrechtecken,
- Matching mehrerer Fenster derselben Klasse,
- Bereinigung von Desktop-Startbefehlen,
- Adapter fuer Chromium, VS Code und `foot`,
- Verhalten bei fehlenden oder geaenderten Monitoren.

### Integrationstests

- Speichern und Laden eines Workspaces mit zwei Fenstern,
- drei ungleich breite gekachelte Fenster,
- verschachtelte horizontale und vertikale Splits,
- mehrere `foot`-Fenster,
- mehrere Chromium-Fenster in einem Prozess,
- langsam startende und fehlende Anwendung,
- Abbruch waehrend einer Wiederherstellung,
- automatisches Laden nach Start der Omarchy-Shell.

### Manuelle Abnahme

Das aktuelle Referenzszenario besteht aus drei Workspaces und acht Fenstern auf einem Monitor im `dwindle`-Layout. Nach einer frischen Anmeldung muessen dieselben Anwendungen auf denselben Workspaces mit denselben wesentlichen Teilungsverhaeltnissen erscheinen. Einzelne nicht wiederherstellbare interne Anwendungszustaende muessen transparent ausgewiesen werden.

## Entwicklungsphasen

### Phase 1: Plugin-Grundgeruest

- Manifest, Bar-Icon, Panel und Hintergrunddienst,
- CLI-Grundstruktur und XDG-Verzeichnisse,
- Profil-Liste und leere Zustandsanzeigen,
- Plugin-Validierung und Basistests.

### Phase 2: Speichern

- Hyprland-Zustand erfassen,
- generische Startbefehle bestimmen,
- Profile atomar schreiben,
- Speichern, Aktualisieren, Umbenennen und Loeschen im Panel.

### Phase 3: Wiederherstellen

- Event-basierter Programmstart,
- Fenstermatching,
- Workspace-Zuordnung,
- Floating-Geometrie,
- Fortschritt, Abbruch und Fehlerzusammenfassung.

### Phase 4: Exaktes Tiling

- `dwindle`-Baum rekonstruieren,
- Einfuegereihenfolge und Split-Richtung wiederherstellen,
- Teilungsverhaeltnisse angleichen,
- Geometrie nach Abschluss validieren.

### Phase 5: Anwendungsadapter und Autostart

- Chromium, Visual Studio Code und `foot`,
- Standardprofil, letztes Profil und Startauswahl,
- Verhalten bei geaenderter Monitorkonfiguration.

### Phase 6: Stabilisierung

- Integrations- und Regressionstests,
- Diagnoseansicht und Logbereinigung,
- Installations- und Benutzeranleitung,
- Vorbereitung einer veroeffentlichbaren Plugin-ID.

## Definition der ersten fertigen Version

Die erste Version gilt als fertig, wenn:

- das Plugin mit Omarchy 4.x validiert und aktiviert werden kann,
- ein Setup ueber die Bar gespeichert und geladen werden kann,
- mehrere gleichartige Fenster korrekt zugeordnet werden,
- Workspace-Zuordnung und `dwindle`-Teilungsverhaeltnisse im Referenzszenario wiederhergestellt werden,
- ein Setup optional nach der Anmeldung geladen werden kann,
- Fehler keine Daten verlieren und im Panel sichtbar sind,
- die Grenzen bei internem Anwendungszustand dokumentiert bleiben.
