# Lebenszyklus: Dienst, Tray und Apps

Stand: 2026-10-22. Konzept zur Freigabe mit Issue #279; Aufgaben in [tasks/apps.md](tasks/apps.md).

Die Apps der Suite sollen sich wie normale Programme verhalten: jede mit eigenem Icon, direkt startbar
über Finder, Spotlight, Dock, Startmenü oder Launcher, ohne Umweg über den Tray, und normal zu
schließen. Dieses Dokument legt fest, wer was startet, was beim Schließen passiert und warum die Apps
eigenständige Programme bleiben.

## Ausgangslage (geprüft 2026-10-22)

- **Die Apps sind dünne Clients.** Kalender, Aufgaben, Eingang, Assistent und Steuerzentrale haben keine
  eigenen Daten und keine eigene Logik; alles geht als `torro_ipc::Request` an den Dienst
  (`torroworkplace daemon`, [api.md](api.md)). Ohne Dienst zeigen sie nur einen Fehler.
- **Die Apps starten den Dienst schon selbst**, aber nur unter Unix: `torro_gui::source::ensure_core`
  startet `torroworkplace daemon`, wenn niemand am Socket antwortet. Gesucht wird das Programm neben der
  App, sonst im `PATH`. In getrennten macOS-Bündeln liegt es weder dort noch im `PATH` einer GUI-App;
  unter Windows startet keine App den Dienst. Dasselbe tun Steuerzentrale, Vorlagen-Dialog und jeder
  MCP-Server (`torroworkplace mcp`, höchstens alle 10 s).
- **Der Dienst startet die Apps als nackte Programme** (`apps.open_app`, Tray, App-Umschalter):
  `torrocalendar` neben dem Dienst oder aus dem `PATH`. Auf macOS hat so ein Prozess keine Bundle-ID;
  Dock und App-Umschalter zeigen das Terminal-Symbol und den Programmnamen.
- **„Schon offen? Dann nach vorn holen“ greift nur unter Hyprland.** `Core::open_app` sucht ein Fenster
  mit der Klasse `torro-calendar`. AeroSpace meldet aber die Bundle-ID bzw. den Programmnamen, GlazeWM
  den Prozessnamen. Auf macOS und Windows startet deshalb jeder Klick im Tray oder im App-Umschalter
  eine weitere Instanz. Das passt zur Beobachtung vom 2026-10-21: Die Apps „öffneten sich immer
  wieder“; geschlossen wurde jeweils nur eine von mehreren.
- **Startschleife.** Scheitert der Dienst nach dem Start (beobachtet: Socket-Pfad zu lang), hat er die
  Einrichtung (`torroworkplace ui --setup`) schon geöffnet. Die startet ihrerseits einen Dienst, der
  wieder die Einrichtung öffnet. Beim Nachstellen entstanden so in wenigen Sekunden rund 70 Prozesse.
- **Beenden** per ⌘Q bzw. Dock › Beenden funktioniert für eine einzelne App (nachgestellt mit einem
  Quit-Ereignis, mit und ohne Start über den Dienst).
- **Bündel und Icons** gibt es nur für Workplace, Kalender und Aufgaben, alle Icons sind Platzhalter.
  Eingang (Hub, Mail, App Feed) und Assistent haben weder `.app` noch `.desktop` noch `.ico`.

## Entscheidungen

1. **Eigenständige Apps.** Jede App ist ein eigenes Programm mit eigenem Bündel bzw. eigener
   `.desktop`-Datei und eigenem Icon und bleibt ein dünner Client des Dienstes. Verworfen: eine App mit
   mehreren Fenstern. Sie hätte ein einziges Dock-Icon (gegen den Wunsch) und hieße großen Umbau, weil
   heute jedes Fenster ein eigener Prozess ist.
2. **Der Eingang wird drei Apps**: Notification Hub, Mail und App Feed, je mit eigenem Programm
   (`torroinbox`, `torro-inbox-mail`, `torro-inbox-notifications`), eigenem Bündel, eigener
   `.desktop`-Datei und eigenem Icon. Der Code bleibt einer: `torroinbox` wird Bibliothek, die beiden neuen
   Programme starten sie mit fester Fensterart. Nach außen ändert sich nichts: `apps.list_apps` nennt
   weiter `torroinbox` als Programm.
3. **Der Dienst bleibt.** Er startet beim Anmelden, wenn „Start at login“ an ist, sonst mit der ersten
   App. Schließt die letzte App, läuft er im Tray weiter: Erinnerungen, Benachrichtigungen, Sync und MCP
   brauchen keine offene App. Er endet nur über Tray › Quit bzw. `ctl quit`.
4. **Quit gilt.** Nach Tray › Quit startet kein MCP-Server den Dienst von selbst neu; erst ein Mensch,
   der eine App öffnet, oder die nächste Anmeldung. Verworfen: Neustart durch Agenten wie bisher. So
   hätte Quit keine Wirkung, solange eine Agenten-Sitzung läuft. Das gilt nur für ein Beenden durch
   einen Menschen (Tray › Quit, `ctl quit`). Endet der Dienst anders (Signal, `launchctl`/`systemctl
   stop`, Absturz), startet ein MCP-Server ihn wie bisher neu.
5. **Eine Instanz je App.** Ein zweiter Start ohne Ziel holt das offene Fenster nach vorn und beendet
   sich, auf allen Systemen und auf jedem Startweg (Launcher, Tray, App-Umschalter, Terminal). Das
   erledigt die App selbst über einen kleinen lokalen Kanal je Fensterart, nicht der Dienst über den
   Fenstermanager. Mit Ziel (`--task=…`, `--message=…`, Verfassen, Chat-Fenster) öffnet weiter ein
   eigenes Fenster wie heute.
6. **macOS: Start über die Bündel.** Der Dienst startet eine App über ihr Bündel, also mit Bundle-ID,
   Icon und Aktivierung durch das System. Die App startet den Dienst aus dessen Bündel. Gesucht wird
   neben dem eigenen Bündel, in `/Applications` und in `~/Applications`; erst danach neben dem Programm
   bzw. im `PATH` (Entwicklung, Homebrew).
7. **macOS: Der Dienst bekommt ein eigenes Agenten-Bündel** „Torro Workplace Service.app“ (ohne
   Dock-Icon), eingebettet in „Torro Workplace.app“ (die Steuerzentrale). Grund: Liefe der Dienst aus
   demselben Bündel wie die Steuerzentrale, würde ein Doppelklick auf „Torro Workplace“ bei laufendem
   Dienst nur den Dienst wecken, und kein Fenster ginge auf. Verworfen: den Weckruf im Dienst selbst
   abfangen. Das bräuchte nativen AppKit-Code mit `unsafe`.
8. **Windows:** Die Apps starten den Dienst auch dort selbst. Startmenü-Einträge kommen über das
   Scoop-Manifest der Suite (`shortcuts`). Windows gruppiert die Taskleiste nach `.exe` und zeigt das
   beim Bauen eingebettete Icon; deshalb bekommen Mail und App Feed eigene Programme (Entscheidung 2),
   je in einem eigenen Crate, denn `winresource` bettet ein Icon in alle Programme eines Crates ein.
   Verworfen: eine `.exe` mit eigener App-ID je Fensterart (`SetCurrentProcessExplicitAppUserModelID`).
   Das bräuchte nativen Code mit `unsafe` und Startmenü-Verknüpfungen mit dieser ID, die Scoop nicht
   anlegen kann; ohne sie bliebe das Anheften kaputt.
9. **Icons:** Ein Agent entwirft ein einheitliches Set für alle Apps mit Vorschauseite unter
   `docs/prototypes/icons/`, der Mensch wählt; daraus entstehen `.icns`, `.ico` und die SVGs für Linux.

## Wer startet was

| Wer | startet | wann |
|---|---|---|
| Anmeldung (LaunchAgent, XDG-Autostart, `Run`-Schlüssel) | den Dienst | „Start at login“ an |
| jede App, Steuerzentrale, Vorlagen-Dialog | den Dienst | beim eigenen Start, wenn er nicht antwortet |
| MCP-Server (`torroworkplace mcp`) | den Dienst | wenn er nicht antwortet, nicht nach Quit durch einen Menschen |
| der Dienst | Apps | Tray, `apps.open_app`, Vorlagen, Erinnerungsfenster |
| der Dienst | Einrichtung | erster Start ohne Konto, erst wenn sein Socket antwortet |

Kein Programm der Suite startet den Dienst, wenn das in den letzten Sekunden schon jemand versucht
hat. So kann keine Startschleife entstehen.

## Je System

| | macOS | Linux (Hyprland) | Windows |
|---|---|---|---|
| Starter | `.app` in `/Applications` (Finder, Spotlight, Dock) | `.desktop` (Launcher, `StartupWMClass` = Fensterklasse) | Startmenü (Scoop `shortcuts`) |
| Identität | Bundle-ID `io.github.mahype.<fensterklasse>` | Wayland-App-ID = Fensterklasse | eigene `.exe` je App mit eingebettetem Icon |
| Dienst | Agenten-Bündel im Workplace-Bündel | `torroworkplace daemon` (systemd, `exec-once` oder erste App) | `torroworkplace.exe daemon` |
| zweiter Start | System aktiviert das Bündel; sonst lokaler Kanal der App | lokaler Kanal der App | lokaler Kanal der App |
| Beenden | ⌘Q, Dock › Beenden, rotes Fenster-Symbol | Fenster schließen | Fenster schließen |

## Folgen

- **Paketierung:** sieben Bündel statt drei, dazu das Agenten-Bündel des Dienstes; zwei Programme mehr
  im Release (`torro-inbox-mail`, `torro-inbox-notifications`), jedes etwa so groß wie der Eingang. Jedes wird
  signiert und notarisiert; der Workflow wird länger, aber nicht anders.
- **Updates** kommen weiter für die ganze Suite über die Paketmanager; Apps und Dienst gleicher Version
  bleiben die Regel, das Protokoll ist versioniert ([api.md](api.md)).
- **API und MCP** ändern sich nicht: `apps.open_app` bleibt; den Dienst starten ist kein Befehl,
  sondern ein lokaler Prozessstart, und `system.quit` bleibt nach „Was nie nach außen geht“ ohne
  MCP-Tool.
