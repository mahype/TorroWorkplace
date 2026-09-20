# Produktanforderungen

## Ziel

Workspace Setups soll auf Omarchy den aktuell sichtbaren Desktopzustand als benanntes Setup speichern und nach einer Anmeldung oder auf Anforderung moeglichst originalgetreu rekonstruieren.

Das Produkt richtet sich an Personen, die wiederkehrende Arbeitsumgebungen verwenden, beispielsweise Entwicklung, Kommunikation oder Freizeit, und diese nicht nach jedem Neustart manuell aufbauen wollen.

## Begriffe

- **Setup:** Gespeicherte Beschreibung von Anwendungen, Fenstern, Workspaces, Monitoren und Layout.
- **Speichern:** Ein neues Setup aus dem aktuellen Hyprland-Zustand erzeugen.
- **Aktualisieren:** Ein vorhandenes Setup mit dem aktuellen Zustand ueberschreiben.
- **Laden:** Anwendungen starten und deren Fenster anhand des Setups anordnen.
- **Anwendungsadapter:** Optionale programmspezifische Logik, die mehr Zustand als ein generischer Prozessstart wiederherstellen kann.

## Hauptfunktionen

### Setup speichern

Das Plugin muss:

- einen Namen fuer das Setup abfragen,
- Monitore, Workspaces und normale Anwendungsfenster erfassen,
- Startinformationen fuer jede Anwendung bestimmen,
- Kachelstruktur und Teilungsverhaeltnisse speichern,
- Floating-Positionen und -Groessen speichern,
- unvollstaendig erkannte Anwendungen vor dem Speichern anzeigen,
- ein vorhandenes gleichnamiges Setup nur nach Bestaetigung ersetzen.

### Setup laden

Das Plugin muss:

- die Monitorkonfiguration vor dem Laden pruefen,
- Anwendungen kontrolliert und in der notwendigen Reihenfolge starten,
- auf neu erscheinende Fenster warten,
- Fenster eindeutig einem gespeicherten Eintrag zuordnen,
- Fenster auf gespeicherte Workspaces und Monitore verschieben,
- Kachelstruktur, Teilungsverhaeltnisse und Fensterzustaende anwenden,
- Fortschritt und Fehler im Panel darstellen,
- bei einzelnen Fehlern mit den verbleibenden Fenstern fortfahren.

### Setups verwalten

Das Bar-Panel bietet:

- Liste aller Setups,
- Laden,
- aktuellen Zustand als neues Setup speichern,
- vorhandenes Setup aktualisieren,
- Umbenennen,
- Loeschen mit Bestaetigung,
- Anzeige von Fenster- und Workspace-Anzahl,
- Kennzeichnung des Standard- und zuletzt geladenen Setups.

### Verhalten beim Anmelden

Folgende Modi werden angeboten:

- nichts automatisch laden,
- ein festes Standard-Setup laden,
- das zuletzt verwendete Setup laden,
- nach der Anmeldung eine Setup-Auswahl anzeigen.

Der Hintergrunddienst darf erst laden, wenn Hyprland und die Monitore bereit sind.

## Bedienoberflaeche

Workspace Setups erscheint als einzelnes Icon in der Omarchy-Bar. Ein Linksklick oeffnet das Panel.

Beispiel:

```text
WORKSPACE SETUPS

● Entwicklung
  3 Workspaces · 8 Fenster

  Laden       Aktualisieren
  Umbenennen  Loeschen

+ Aktuellen Zustand speichern

Beim Anmelden:
[ Letztes Setup laden      v ]
```

Beim Laden zeigt das Panel den Status pro Anwendung:

```text
Entwicklung wird geladen

✓ Chromium        Workspace 1
✓ Terminal oben   Workspace 1
✓ Visual Studio   Workspace 3
! Terminal 3      Zeitueberschreitung

7 von 8 Fenstern wiederhergestellt
```

## Laden bei vorhandenen Fenstern

Das Produkt unterscheidet drei Modi:

- **Leere Sitzung:** Empfohlener und zuverlaessigster Modus.
- **Ergaenzen:** Vorhandene Fenster bleiben bestehen; Setup-Fenster werden hinzugefuegt.
- **Ersetzen:** Vorhandene normale Anwendungsfenster werden nur nach ausdruecklicher Bestaetigung geschlossen.

Der Standardmodus darf niemals ungefragt Fenster schliessen.

## Wiederherstellbarer Zustand

Das Setup speichert, soweit verfuegbar:

- Monitorname, Aufloesung, Position und Skalierung,
- Workspace-ID oder -Name,
- Hyprland-Layout,
- Fensterklasse und urspruengliche Fensterklasse,
- Startbefehl als Argumentliste,
- Position und Groesse,
- Tiling- oder Floating-Zustand,
- Kachelbaum und Teilungsverhaeltnisse,
- Vollbild- und Pin-Zustand,
- Anwendungstitel als zusaetzliches Zuordnungsmerkmal,
- Projektpfad oder Arbeitsverzeichnis ueber Anwendungsadapter.

## Grenzen

- Chromium muss Tabs grundsaetzlich selbst wiederherstellen. Eine spaetere Browser-Erweiterung kann einzelne Fenster und URLs erfassen.
- Visual Studio Code stellt Editorzustand und nicht gespeicherte Dateien ueber seine eigene Sitzungslogik wieder her.
- `foot` kann mit einem Arbeitsverzeichnis gestartet werden; laufende Shell-Prozesse benoetigen `tmux` oder `zellij`.
- Nicht gespeicherte Dokumente und beliebiger Prozessspeicher sind nicht Teil eines Setups.
- Pixelgenaue Wiederherstellung wird fuer dieselbe Monitorkonfiguration angestrebt. Bei geaenderter Aufloesung wird eine angepasste oder teilweise Wiederherstellung notwendig.

## Qualitaetsziele

- Ein fehlendes Programm darf den restlichen Ladevorgang nicht blockieren.
- Mehrere Fenster derselben Anwendung muessen unterscheidbar sein.
- Ein Ladeprozess muss abbrechbar sein.
- Gleichzeitige Ladeprozesse muessen verhindert werden.
- Profile muessen versioniert und vor inkompatiblen Aenderungen migrierbar sein.
- Fehler muessen im Panel verstaendlich und in technischen Logs detailliert erscheinen.

## Nichtziele der ersten Version

- Speicherabbilder laufender Prozesse,
- Wiederherstellung nicht gespeicherter Dokumente,
- Synchronisation zwischen mehreren Rechnern,
- Unterstuetzung anderer Desktopumgebungen oder Window Manager,
- automatische Erfassung aller Chromium-Tabs,
- beliebige Umrechnung komplexer Layouts zwischen unterschiedlichen Monitorlandschaften.
