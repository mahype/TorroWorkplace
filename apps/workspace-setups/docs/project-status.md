# Projektstand und Entscheidungen

Stand: 25. September 2026 (Version 0.2.0)

## Neuausrichtung in Version 0.2

Version 0.1 speicherte alle Workspaces als Profil und stellte sie an denselben Nummern wieder her. Das hat sich fuer den Alltag nicht bewaehrt. Seit 0.2 gilt:

- Gespeichert wird nur der aktive Workspace, als **Workspace-Vorlage** (englisch *workspace template*).
- Geladen wird in den aktiven Workspace, unabhaengig von dessen Nummer.
- Ist der Workspace nicht leer, fragt das Plugin: Schliessen & laden, Hinzufuegen oder Abbrechen.
- Programme, die nur einmal laufen koennen, werden von anderen Workspaces herangeholt; mehrfach startbare Programme werden neu geoeffnet. Die Strategie ist je Fenster einstellbar.
- Tastenkuerzel: `SUPER + F1..F10` laden, `SUPER + SHIFT + F1..F10` speichern mit Warnung bei belegter Taste. `SUPER + F<n>` ist in Omarchy 4 nicht belegt.
- „Beim Start wiederherstellen“ ist eine Einstellung je Vorlage mit festem Workspace; der Startlauf passiert einmal pro Hyprland-Sitzung.
- Profile aus 0.1 werden verworfen (Schema 1 wird nicht mehr gelesen).
- Die Oberflaeche ist englisch mit deutscher Uebersetzung; Standard ist die Systemsprache.

## Weiterhin gueltig

- Eigenstaendiges Repository, Zielplattform Omarchy 4.x mit Hyprland im `dwindle`-Layout.
- Trennung zwischen QML-Oberflaeche und testbarem Python-Backend ohne Fremdabhaengigkeiten.
- Lua-Dispatcher fuer den Hyprland-Konfigurationsprovider von Omarchy 4.
- Event-basierte Fenstererkennung mit abfragendem Fallback.
- Adapter fuer Chromium-PWAs, VS-Code-Projekte und `foot`-Arbeitsverzeichnisse.

## Verifikation

- 60 automatisierte Tests, `omarchy plugin validate` erfolgreich.
- Live in Omarchy 4 (Hyprland, ein Monitor `DP-1`): Panel und Rueckfrage-Dialog laden ohne QML-Fehler; Speichern per IPC `saveSlot`; Laden auf belegtem Workspace zeigt die Rueckfrage; drei `foot`-Fenster mit verschachteltem Split (35 % / 50 %) auf Workspace 9 in unter einer Sekunde aufgebaut; `replace` ersetzt nur die Fenster des Ziel-Workspace.
- Noch nicht live geprueft: Tastenkuerzel nach `install-bindings`, Herholen eines Einzelinstanz-Programms, Startlauf nach frischer Anmeldung.

## Bewusst spaeter

- Vordergrundprogramme auch fuer Alacritty, Kitty und Ghostty erkennen; Terminal-Befehl im Panel bearbeitbar machen.
- Zuletzt aktiven Browser-Tab wiederherstellen; Firefox-Tabs.
- Exakteres Einfuegen beim Hinzufuegen zu vorhandenen Fenstern.
