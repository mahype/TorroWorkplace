# Projektstand und Entscheidungen

Stand: 19. September 2026

## Implementiert

- Projektname: **Workspace Setups**.
- Eigenstaendiges Repository statt Integration in LightSync.
- Ausschliessliche Zielplattform der ersten Version: Omarchy 4.x mit Hyprland.
- Auslieferung als ein Omarchy-Plugin mit Bar-Widget, Panel und Hintergrunddienst.
- Technische Trennung zwischen QML-Oberflaeche und testbarem Python-Backend.
- Ein Setup rekonstruiert den Desktopzustand, ist aber kein Prozess- oder Speicherabbild.
- Der Standard-Ladevorgang schliesst keine bestehenden Fenster.
- Profile werden ausserhalb des Plugin-Quellverzeichnisses in XDG-Verzeichnissen gespeichert.
- Erstunterstuetzung fuer Chromium, Visual Studio Code und `foot`.
- Fokus auf `dwindle` und eine gleichbleibende Monitorkonfiguration.
- Lua-Dispatcher fuer den in Omarchy 4 verwendeten Hyprland-Konfigurationsprovider.
- Event-basierte Fenstererkennung mit abfragendem Fallback.
- Drei sichere Lademodi: `empty`, `add` und bestaetigtes `replace`.
- Startdienst fuer `none`, festes Profil, letztes Profil oder Auswahl.
- 44 automatisierte Tests fuer Speicherung, Sicherheit, Layout, IPC und Wiederherstellung.
- Unterstuetzung fuer numerische und benannte Hyprland-Workspaces.
- Sichere Monitor-Fallbacks und garantiertes Aufraeumen temporaerer Staging-Workspaces.
- Chromium-PWA-Startargumente sowie VS-Code-Projekt- und Terminal-Verzeichniserkennung.

## Referenzumgebung

Die Planung wurde gegen folgende lokale Umgebung erstellt:

- Omarchy `4.0.4-1`
- Hyprland `0.56.2`
- Omarchy Shell als Quickshell-Prozess
- ein Monitor `DP-1`
- drei Workspaces im `dwindle`-Layout
- acht Fenster aus Chromium, Visual Studio Code und `foot`

Die Referenzumgebung dient der ersten manuellen Abnahme und ist keine dauerhaft festgeschriebene Produktgrenze.

## Bewusst spaeter

- Endgueltige veroeffentlichbare Plugin-ID.
- Name und Symbol des Bar-Icons.
- Genaue UX fuer Profile, deren Monitorkonfiguration nicht mehr vorhanden ist.
- Ob Profilvorschaubilder nach dem MVP aufgenommen werden.
- Welche Chromium-Sitzungseinstellungen das Plugin erkennen oder nur dokumentieren soll.
- Optionale Chromium-Erweiterung zur expliziten Speicherung einzelner Tabs.
- Umrechnung komplexer Profile auf grundlegend andere Monitorlandschaften.

## Verifikation

- Lokale Installation und Aktivierung in Omarchy 4.0.4 erfolgreich.
- Bar-Widget, Panel und Hintergrunddienst laden ohne Plugin-eigene QML-Fehler.
- Reale Sitzung als `Aktuelles Setup` mit drei Workspaces und sieben Fenstern gespeichert.
- Sicherer `empty`-Modus verweigert die Wiederherstellung bei offenen Fenstern, ohne Fenster zu veraendern.
- Vollstaendige Wiederherstellung ist fuer die naechste tatsaechlich leere Anmeldung vorgesehen, da ein Live-Test sonst die laufende Arbeit zerstoeren wuerde.
