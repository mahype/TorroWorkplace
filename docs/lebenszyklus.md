# Lebenszyklus: Dienst, Tray und Apps

Stand: 2026-10-22, umgesetzt mit Issue #279 (Aufgaben AP.1–AP.8 in [tasks/apps.md](tasks/apps.md)). Das Dokument
beschreibt den gebauten Stand; die Abnahme von Hand steht unter „Von Hand prüfen“.

Die Apps der Suite verhalten sich wie normale Programme: jede mit eigenem Icon, direkt startbar
über Finder, Spotlight, Dock, Startmenü oder Launcher, ohne Umweg über den Tray, und normal zu
schließen. Dieses Dokument hält fest, wer was startet, was beim Schließen passiert und warum die Apps
eigenständige Programme bleiben.

## Vorher (geprüft 2026-10-22, vor #279)

So war es, bevor #279 umgesetzt wurde. Was daraus wurde, steht unter „Entscheidungen“ und „Wie gebaut“.

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
| jede App, Steuerzentrale, Vorlagen-Dialog (die erste, die ein Mensch öffnet) | den Dienst | beim eigenen Start, wenn er nicht antwortet |
| MCP-Server (`torroworkplace mcp`) | den Dienst | wenn er nicht antwortet, nicht nach Quit durch einen Menschen |
| der Dienst | Apps | Tray, `apps.open_app`, Vorlagen, Erinnerungsfenster |
| der Dienst | Einrichtung | erster Start ohne Konto, erst wenn sein Socket antwortet |

Kein Programm der Suite startet den Dienst, wenn das in den letzten 10 Sekunden schon jemand versucht
hat (Marke `core-start`). So kann keine Startschleife entstehen.

## Wie gebaut

Was bei der Umsetzung (AP.1–AP.7) zusätzlich entschieden wurde.

- **Fenster wiederfinden: der Instanz-Kanal entscheidet.** Der Dienst holt ein Fenster nur dann über den
  Fenstermanager nach vorn, wenn es genau die Fensterklasse des Eintrags trägt (`torro-calendar`); das liefert
  nur Hyprland. AeroSpace nennt alle Fenster eines Programms nach der Bundle-ID bzw. dem Programmnamen,
  GlazeWM nach dem Prozess. Dort wäre auch ein Nebenfenster (Verfassen, Chat, Vorlagen-Dialog, `--task`)
  ein Treffer und käme statt des Hauptfensters nach vorn. Ohne genaue Klasse startet der Dienst die App
  deshalb neu, und der neue Prozess fragt den Instanz-Kanal: Läuft das Hauptfenster, schickt er `show`
  und endet; läuft nur ein Nebenfenster, öffnet er das Hauptfenster. Für die Steuerzentrale gilt dasselbe.
  `apps.list_apps` antwortet weiter mit `torroinbox` für alle drei Eingangs-Apps.
- **Marke `core-start`.** `ensure_core` schreibt den Zeitpunkt des Versuchs in das State-Verzeichnis des Dienstes,
  neben `core.sock`, und startet nichts, wenn die Marke jünger als 10 Sekunden ist. Der Dienst öffnet die
  Einrichtung erst, wenn sein Socket auf `system.ping` antwortet; scheitert der Socket, öffnet sich nichts.
  Antwortet der Socket, löscht der Dienst die Marke: Wer ihn kurz nach dem Start über Tray › Quit beendet und
  gleich darauf eine App öffnet, startet ihn wieder. Ein Dienst, der nie antwortet, lässt die Marke stehen.
- **Marke `quit`.** Beendet ein Mensch den Dienst, schreibt der Dienst die Marke `quit` (0600, nur der
  Zeitpunkt). Das geschieht an einer Stelle für beide Wege, Tray › Quit und `system.quit` (`ctl quit`, nur mit
  dem App-Schlüssel). Beim nächsten Start löscht er sie. Solange sie steht, startet der MCP-Server nichts und
  antwortet mit dem Code `offline` und einer eigenen Meldung. Apps und Steuerzentrale achten nicht auf die
  Marke: Wer eine App öffnet, will den Dienst. Endet der Dienst anders, entsteht keine Marke.
- **Instanz-Kanal** (`torro_gui::instance`). Je Fensterklasse ein eigener Kanal: unter Unix ein Socket
  `<state>/<fensterklasse>.instance` (0600 in einem 0700-Verzeichnis), unter Windows eine Named Pipe
  `\\.\pipe\torro-<fensterklasse>-<SID>`. Die Pipe entsteht mit `torro_ipc::pipe::user_only_listener`, also
  mit denselben Rechten wie `core.sock` (nur der Benutzer, keine entfernten Clients); der Client verbindet mit
  der Security-QoS „Identification“. Der Kanal kennt nur die Zeile `show`; Argumente, Ziele und Pfade gehen nie
  darüber. Wer den Namen besetzt, kann den Start einer App verhindern, mehr nicht. Ist der Socket-Pfad zu lang
  (104 Byte auf macOS), startet die App ohne Kanal.
- **Starts mit Ziel haben keinen Kanal** und öffnen ein eigenes Fenster: `--task`, `--message`, `--compose`,
  `--chat`, `--listen`, `--voice-*`, `--reminders`, `--settings`, `--template`, `--demo`. Im Eingang zählen
  auch `--lane` und `--source` als Ziel: Vorlagen bringen Fenster einer Spur oder Quelle zurück, und das sind
  eigene Fenster. Ein einfacher Start von Hub, Mail oder App Feed hat dagegen je seinen Kanal.
- **Eingang als drei Programme.** `torroinbox` ist Bibliothek und Programm; `torro-inbox-mail` und
  `torro-inbox-notifications` setzen `--view` und starten die Bibliothek. Der Dienst startet Mail und App Feed
  über ihr eigenes Programm, wenn es da ist, sonst über `torroinbox --view …`. Das Verfassen bleibt bei
  `torroinbox`.
- **macOS: Start über das Bündel.** Der Dienst startet eine App immer als neuen Prozess mit
  `open -n -a <absoluter Bündelpfad>`, nie mit dem Anzeigenamen; mit Ziel mit `open -n -a <Pfad> --args …`.
  Ohne `-n` würde das System irgendeinen laufenden Prozess des Bündels aktivieren, auch ein Nebenfenster;
  ob das Hauptfenster schon läuft, sagt der Instanz-Kanal. Ohne Bündel, unter Windows und Linux startet der
  Dienst das Programm. Gesucht wird neben dem eigenen Bündel, in
  `/Applications` und in `~/Applications`, danach neben dem Programm und im `PATH`. Ein Bündel in
  `/Applications` geht also einem Entwicklungs-Build vor: Wer entwickelt und ein Release installiert hat,
  startet das Release.
- **macOS: Steuerzentrale aus dem Bündel.** Einrichtung, Steuerzentrale und Vorlagen-Dialog startet der Dienst
  aus „Torro Workplace.app“, nie aus seinem eigenen (Service-)Bündel, immer mit `-n`. Aus dem Service-Bündel
  liefen sie ohne Dock-Icon und mit der Bundle-ID des Dienstes; eine zweite Steuerzentrale übergibt über den
  Instanz-Kanal an die erste.
- **Windows.** Jede App startet den Dienst losgelöst und ohne Konsole (`DETACHED_PROCESS |
  CREATE_NEW_PROCESS_GROUP`). Die Programme sind GUI-Programme ohne Konsolenfenster; ein Fehler beim Start
  landet in einer Datei im State-Verzeichnis des Eingangs statt in einer Konsole.
- **Bündel und Pakete.** Neun macOS-Bündel (sieben Apps, das Service-Bündel `io.github.mahype.torro-workplace-service`
  in „Torro Workplace.app“ unter `Contents/Library/LoginItems/`, wo Apple Hilfs-Apps erwartet, die Erinnerung „Torro Reminder.app“), eine `.desktop`-Datei je Eintrag, AUR-Pakete
  für alle sieben Programme (die Suite hängt an allen, Mail und App Feed an `torroinbox-bin`), Homebrew-Formeln
  und ein Scoop-Manifest für die ganze Suite mit sieben Startmenü-Einträgen. Details:
  [release.md](release.md), [../packaging/README.md](../packaging/README.md).
- **Icons.** Variante „red“ aus `docs/prototypes/icons/red/`. macOS mit Big-Sur-Rand und Schatten, Linux und
  Windows flach mit 4 % Rand.
- **Signatur-Kennung des Dienstes.** Im Release trägt der Dienst die Kennung des Service-Bündels
  (`io.github.mahype.torro-workplace-service`). Der Schlüsselbund fragt deshalb einmal neu; bisher gab es kein
  Bündel-Release, also keine Kennung, die sich ändert. Entwicklungs-Builds behalten
  `io.github.mahype.dev.torroworkplace`.

## Je System

| | macOS | Linux (Hyprland) | Windows |
|---|---|---|---|
| Starter | `.app` in `/Applications` (Finder, Spotlight, Dock) | `.desktop` (Launcher, `StartupWMClass` = Fensterklasse) | Startmenü (Scoop `shortcuts`) |
| Identität | Bundle-ID `io.github.mahype.<fensterklasse>` | Wayland-App-ID = Fensterklasse | eigene `.exe` je App mit eingebettetem Icon |
| Dienst | Agenten-Bündel im Workplace-Bündel | `torroworkplace daemon` (systemd, `exec-once` oder erste App) | `torroworkplace.exe daemon` |
| Tray, App-Umschalter, `apps.open_app` | neuer Prozess (`open -n -a`), dann lokaler Kanal der App | Fenster mit genauer Klasse über Hyprland nach vorn, sonst neuer Prozess und lokaler Kanal | neues Programm, dann lokaler Kanal der App |
| zweiter Start | lokaler Kanal der App | lokaler Kanal der App | lokaler Kanal der App |
| Beenden | ⌘Q, Dock › Beenden, rotes Fenster-Symbol | Fenster schließen | Fenster schließen |

## Von Hand prüfen

Die Abnahme von AP.8 macht ein Mensch; Tests und Doku ersetzen sie nicht. Nichts davon startet ein Agent.

Offen ist, ob das Nach-vorn-Holen über den Instanz-Kanal unter macOS und Windows gegen die Aktivierungsregeln
des Systems ankommt: Die laufende App ruft nur `gain_focus`; das System darf stattdessen das Dock-Icon hüpfen
lassen bzw. das Taskleisten-Symbol blinken lassen. Das zeigt erst die Prüfung von Hand.

### macOS

Bündel bauen: `cargo build --workspace`, dann `scripts/macos-dev-bundles.sh ~/Applications/Torro\ Dev`. Das Skript
signiert mit „Torro Development“ (`scripts/macos-dev-cert.sh`), sonst ad hoc. Dienst vorher beenden
(Tray › Quit), danach `~/Applications/Torro Dev` im Finder und in Spotlight prüfen.

`open` gibt keine Umgebung weiter: Eine App, die der Dienst über ihr Bündel startet, sieht weder
`TORRO_STATE_HOME` noch andere `TORRO_*`-Variablen. Von Hand also mit den Standardpfaden prüfen, nicht mit
eigenem `TORRO_STATE_HOME`; sonst sprechen Dienst und App über verschiedene Sockets.

- [ ] `plutil -lint` auf alle `Info.plist`; neun Bündel sind da.
- [ ] Jede der sieben Apps aus Finder und Spotlight starten, ohne laufenden Dienst: Das Fenster öffnet, der Dienst
      startet mit (Tray-Icon erscheint).
- [ ] Das Dock zeigt Name und Icon jeder App, nicht das Terminal-Symbol.
- [ ] Zweiter Start (Finder, Spotlight, Dock-Icon) holt das offene Fenster nach vorn, es entsteht keine zweite
      Instanz.
- [ ] Dasselbe aus dem Terminal: `torrocalendar` zweimal, und `torro-inbox-mail` neben `torroinbox` (zwei
      verschiedene Fenster).
- [ ] Tray › „Calendar“ dreimal wählen: genau ein Kalender, beim zweiten und dritten Mal nach vorn geholt
      (neuer Prozess, der über den Instanz-Kanal übergibt und endet).
- [ ] Mit offenem Verfassen-Fenster (bzw. Chat, Vorlagen-Dialog) Tray › „Mail“ (bzw. „Assistant“,
      Steuerzentrale): Das Hauptfenster öffnet sich, nicht das Nebenfenster kommt nach vorn.
- [ ] Unter AeroSpace dasselbe: Kein Nebenfenster kommt statt des Hauptfensters nach vorn.
- [ ] Kommt das Fenster wirklich nach vorn, oder hüpft nur das Dock-Icon? Ergebnis hier festhalten.
- [ ] ⌘Q, rotes Fenster-Symbol und Dock › Beenden schließen die App. Der Dienst bleibt im Tray.
- [ ] Doppelklick auf „Torro Workplace“ bei laufendem Dienst öffnet die Steuerzentrale (nicht nur den Dienst).
- [ ] Eingang: ein Fenster mit `--lane info` bzw. `--source …` öffnet neben dem einfachen Hub-Fenster.
- [ ] Tray › Quit; eine Agenten-Sitzung ruft ein Torro-Tool: Antwort `offline`, der Dienst bleibt aus. Dann eine
      App öffnen: Der Dienst läuft wieder.
- [ ] Ein Release in `/Applications` neben dem Entwicklungs-Build: Der Dienst startet die Apps aus dem Release.
- [ ] Nach dem ersten Release-Dienst fragt der Schlüsselbund einmal; „Immer erlauben“ gilt danach.

### Linux (Hyprland)

- [ ] `desktop-file-validate packaging/linux/*.desktop`; die sieben Einträge erscheinen im Launcher, jeder mit
      Icon und Name.
- [ ] Jede App aus dem Launcher ohne laufenden Dienst starten: Der Dienst startet mit.
- [ ] Zweiter Start (Launcher, Terminal, Tray) holt das Fenster nach vorn, auch aus dem App-Umschalter.
- [ ] Hyprland erkennt die Fensterklasse (`hyprctl clients`) und Fensterregeln greifen.
- [ ] Fenster schließen lässt den Dienst im Tray laufen; Tray › Quit beendet ihn, ein MCP-Aufruf startet ihn
      nicht neu.

### Windows

- [ ] Scoop-Manifest installieren: sieben Startmenü-Einträge mit Icon.
- [ ] Jede App aus dem Startmenü: ohne Konsolenfenster, der Dienst startet losgelöst mit.
- [ ] Die Taskleiste zeigt für Mail und App Feed ein eigenes Icon und gruppiert sie getrennt vom Hub.
- [ ] Zweiter Start holt das Fenster nach vorn (Named Pipe), keine zweite Instanz; auch aus Tray und
      App-Umschalter unter GlazeWM. Kommt es wirklich nach vorn oder blinkt nur das Taskleisten-Symbol?
- [ ] Die Pipe `\\.\pipe\torro-<fensterklasse>-<SID>` ist nur für den Benutzer zugänglich.
- [ ] Ein Startfehler erscheint in der Datei im State-Verzeichnis des Eingangs, nicht in einer Konsole.

Fehlt ein System, bleibt sein Block im PR als offen stehen.

## Folgen

- **Paketierung:** sieben Bündel plus das Service-Bündel und die Erinnerung statt drei Bündel; zwei Programme
  mehr im Release (`torro-inbox-mail`, `torro-inbox-notifications`), jedes etwa so groß wie der Eingang. Jedes
  wird signiert und notarisiert, das eingebettete Bündel zuerst; der Workflow wird länger, aber nicht anders.
- **Updates** kommen weiter für die ganze Suite über die Paketmanager; Apps und Dienst gleicher Version
  bleiben die Regel, das Protokoll ist versioniert ([api.md](api.md)).
- **API und MCP** ändern sich nicht: `apps.open_app` bleibt; den Dienst starten ist kein Befehl,
  sondern ein lokaler Prozessstart, und `system.quit` bleibt nach „Was nie nach außen geht“ ohne
  MCP-Tool.
