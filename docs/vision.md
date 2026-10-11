# Vision

## Worum es geht

Früher gab es für den Arbeitsalltag „die eine Software“: ein großes Programm mit Kalender,
Aufgaben, Mail und Dashboard in Reitern. Mit einem Tiling-Window-Manager braucht es das nicht
mehr. Der Window Manager *ist* das Dashboard. TorroWorkplace liefert dafür die Bausteine:
**viele kleine, eigenständige Apps**, jede in ihrem eigenen Fenster, die sich frei über den
Desktop verteilen lassen.

- **Im Zentrum steht der Kern `torroworkplace`:** ein Hintergrunddienst mit Icon in der Taskleiste.
  Er hält die Workspaces des Tiling-Managers als Vorlagen fest und baut sie auf Knopfdruck wieder auf.
  Von dort aus startet man später auch die Apps, richtet Konten ein und sieht den Sync-Status.
  Er synchronisiert für alle Apps und ist die eine Anlaufstelle für KI-Agenten.
- **Zuerst Hyprland (Omarchy)**, dann **AeroSpace auf dem Mac** und **GlazeWM unter Windows**.
- **Workspace-Vorlagen** im Kern merken sich, welche Apps wo lagen und was in ihnen offen war, und
  stellen ganze Workspaces per Tastendruck oder beim Start wieder her (nach dem Vorbild von workspace-setups).
- **Zwei Modi:** Im **Tiling-Modus** kachelt der Window Manager die App-Fenster. Wer keinen
  Tiling-Manager hat, nutzt den **Fenster-Modus**: eine Torro-Arbeitsfläche, auf der die Elemente
  der Suite frei angeordnet liegen, mit fertigem Startaufbau. Konzept: [modes.md](modes.md).
- **Ein Befehl installiert alles.**

## Warum gerade jetzt: Arbeiten mit KI

Wer mit mehreren KI-Agenten arbeitet (Claude Code, Codex, …), jongliert mit vielen gleichzeitig
laufenden Dingen: Sitzungen, Branches, Pull Requests, Termine, Aufgaben, Freigaben. Die Suite
soll das **übersichtlicher** machen, auf zwei Wegen:

1. **Für den Menschen:** Jede App beantwortet *eine* Frage auf einen Blick („Was steht heute an?“,
   „Welcher Agent wartet auf mich?“). Sie bleibt dafür sichtbar in ihrer Kachel liegen.
2. **Für die Agenten:** Der Kern hat einen MCP-Server für die ganze Suite. Ein Agent kann eine Aufgabe anlegen,
   einen Termin nachschlagen oder einen Status melden. Mensch und Agent arbeiten auf denselben
   Daten, ohne Copy & Paste.

## Prinzipien

1. **Eine App, eine Frage, ein Fenster.** Lieber zwei kleine Apps als eine mit Reitern.
2. **Offene Standards zuerst.** CalDAV/iCalendar (Nextcloud, iCloud, …) vor proprietären APIs.
   Andere Anbieter kommen als zusätzliche Provider hinzu, nie als Umbau.
3. **Offline zuerst.** Lokaler Cache, Sync im Hintergrund. Die Oberfläche wartet nie auf das Netz.
4. **Ein Kern, viele Apps.** Konten, Sync und MCP liegen einmal im Kern; die Apps sind schlanke
   Clients. Jede hat TUI, später GUI, `status --json` und `ctl`.
5. **Kern zuerst.** Erst der Kern mit Tray-Icon und Workspace-Vorlagen, dann die Apps. Die Apps bekommen eine GUI (Iced); TUIs bleiben möglich.
6. **Feste App-ID.** Jedes Fenster hat eine stabile Klasse (`torro-<app>`), damit Window-Manager-Regeln
   und workspace-setups es zuverlässig wiederfinden.
7. **Leicht.** Zehn Apps gleichzeitig dürfen den Rechner nicht spürbar belasten, deshalb kein Electron.
8. **Plattformübergreifend von Anfang an mitgedacht.** Rust-Kern, keine Linux-only-Abkürzungen im
   Kern. Plattformspezifisches liegt in klar getrennten Backends.
9. **Torro-Designsprache.** Tokens aus `torro-design`, Rot ist Marke und nie Status.
