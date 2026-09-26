# Produktanforderungen

## Ziel

Workspace Setups speichert auf Omarchy einen einzelnen, gerade sichtbaren Workspace als **Workspace-Vorlage** und baut ihn auf Anforderung, per Tastenkuerzel oder nach der Anmeldung auf einem beliebigen Workspace wieder auf.

Das Produkt richtet sich an Personen, die wiederkehrende Arbeitsbereiche verwenden, beispielsweise Entwicklung, Kommunikation oder 3D-Druck, und diese nicht jedes Mal manuell zusammenstellen wollen. Anders als Version 0.1 stellt es nicht mehr alle Workspaces auf einmal an festen Nummern wieder her.

## Begriffe

- **Workspace-Vorlage** (englisch *workspace template*): Gespeicherte Beschreibung der Anwendungen, Fenster und Anordnung eines Workspace.
- **Speichern:** Eine neue Vorlage aus dem aktiven Workspace erzeugen.
- **Aktualisieren:** Eine vorhandene Vorlage mit dem aktiven Workspace ueberschreiben; Einstellungen der Vorlage bleiben erhalten.
- **Laden:** Anwendungen starten oder herholen und ihre Fenster im Ziel-Workspace anordnen.
- **Ziel-Workspace:** Der beim Laden aktive Workspace, beim Start der fuer die Vorlage eingestellte Workspace.
- **Platz (Slot):** Eine der Tasten F1 bis F10, die einer Vorlage zugeordnet werden kann.
- **Strategie:** Wie ein gespeichertes Fenster zurueckkommt: neu oeffnen, herholen oder automatisch.

## Hauptfunktionen

### Vorlage speichern

Das Plugin muss:

- nur die Fenster des aktiven Workspace erfassen,
- Startinformationen fuer jede Anwendung bestimmen,
- Kachelstruktur und Teilungsverhaeltnisse speichern,
- Floating-Positionen relativ zum Monitor speichern,
- fuer jedes Fenster eine Standardstrategie vorschlagen,
- eine gleichnamige Vorlage nur nach Rueckfrage ueberschreiben,
- beim Speichern per `SUPER + SHIFT + F<n>` warnen, wenn auf F<n> bereits eine Vorlage liegt.

### Vorlage laden

Das Plugin muss:

- den Ziel-Workspace zu Beginn festhalten, auch wenn startende Programme den Fokus verschieben,
- bei einem nicht leeren Ziel-Workspace fragen: **Schliessen & laden**, **Hinzufuegen** oder **Abbrechen**,
- je Fenster die Strategie anwenden:
  - **neu:** Programm immer starten,
  - **herholen:** ein bereits offenes, passendes Fenster von einem anderen Workspace verschieben und nur starten, wenn keines existiert,
  - **automatisch:** Programm starten und, wenn in kurzer Zeit kein neues Fenster erscheint, das laufende Fenster herholen,
- auf neu erscheinende Fenster warten und sie eindeutig einem gespeicherten Eintrag zuordnen,
- Kachelstruktur, Teilungsverhaeltnisse und Fensterzustaende anwenden,
- Fortschritt und Fehler im Panel darstellen,
- bei einzelnen Fehlern mit den verbleibenden Fenstern fortfahren.

### Vorlagen verwalten

Das Bar-Panel bietet:

- Liste aller Vorlagen mit Tastenkuerzel- und Start-Kennzeichnung,
- aktiven Workspace als neue Vorlage speichern,
- Hier laden, Aktualisieren, Umbenennen, Loeschen mit Bestaetigung,
- Einstellungen je Vorlage: Tastenkuerzel F1–F10, Beim Start wiederherstellen samt Workspace, Strategie je Fenster,
- Sprache: Systemsprache, Englisch oder Deutsch,
- Knopf zum Aktivieren der Tastenkuerzel.

### Tastenkuerzel

- `SUPER + F1` bis `SUPER + F10` laden die zugeordnete Vorlage in den aktiven Workspace.
- `SUPER + SHIFT + F1` bis `F10` speichern den aktiven Workspace auf diese Taste. Ist die Taste frei, entsteht eine neue Vorlage `Template F<n>`; ist sie belegt, fragt das Panel vor dem Ueberschreiben.
- Rueckfragen oeffnen das Panel; Ergebnisse von Tastenkuerzeln erscheinen zusaetzlich als Desktop-Benachrichtigung.
- Jede Taste gehoert hoechstens einer Vorlage. Das Umhaengen einer belegten Taste erfordert eine Rueckfrage.

### Verhalten beim Anmelden

- Jede Vorlage kann **Beim Start wiederherstellen** mit einem festen Workspace (1–10) aktivieren.
- Nach der Anmeldung werden alle so markierten Vorlagen nacheinander auf ihren Workspace geladen; vorhandene Fenster bleiben erhalten.
- Das passiert einmal pro Hyprland-Sitzung, auch wenn die Omarchy Shell neu gestartet wird.

## Wiederherstellbarer Zustand

Eine Vorlage speichert, soweit verfuegbar:

- Quell-Workspace und Monitorgroesse (zur Information und fuer exakte Groessen),
- Fensterklasse und urspruengliche Fensterklasse,
- Startbefehl als Argumentliste,
- Strategie je Fenster,
- Position relativ zum Monitor und Groesse,
- Tiling- oder Floating-Zustand,
- Kachelbaum und Teilungsverhaeltnisse,
- Vollbild- und Pin-Zustand,
- Fenstertitel als zusaetzliches Zuordnungsmerkmal,
- Projektpfad oder Arbeitsverzeichnis ueber Anwendungsadapter.

## Grenzen

- Die Tabs von Chromium-, Chrome- und Brave-Fenstern werden beim Speichern aus der Sitzungsdatei des Browsers gelesen und beim Laden mit `--new-window` geoeffnet. Das Fenster wird ueber den Titel des aktiven Tabs zugeordnet; der zuletzt aktive Tab wird nicht wiederhergestellt. Firefox wird nicht unterstuetzt.
- Visual Studio Code stellt Editorzustand und nicht gespeicherte Dateien ueber seine eigene Sitzungslogik wieder her.
- `foot` wird mit Arbeitsverzeichnis und, falls es auf der Liste interaktiver Programme steht, mit dem Vordergrundprogramm (`-e herdr`) gestartet. Andere Terminals speichern nur sich selbst.
- Beim Hinzufuegen zu vorhandenen Fenstern ist die Anordnung nur naeherungsweise exakt.
- Auf einem Monitor anderer Groesse werden Teilungsverhaeltnisse uebernommen, feste Groessen gekachelter Fenster aber nicht erzwungen.

## Qualitaetsziele

- Ein fehlendes Programm darf den restlichen Ladevorgang nicht blockieren.
- Mehrere Fenster derselben Anwendung muessen unterscheidbar sein.
- Ein Ladeprozess muss abbrechbar sein; gleichzeitige Ladeprozesse muessen verhindert werden.
- Fenster werden niemals ungefragt geschlossen.
- Vorlagen sind versioniert (Schema 2).
- Die Oberflaeche ist vollstaendig auf Englisch und Deutsch verfuegbar.

## Nichtziele

- Wiederherstellung aller Workspaces auf einmal,
- Speicherabbilder laufender Prozesse oder nicht gespeicherter Dokumente,
- Uebernahme der Profile aus Version 0.1,
- Synchronisation zwischen mehreren Rechnern,
- Unterstuetzung anderer Desktopumgebungen oder Window Manager.
