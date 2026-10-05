# Vision

## Worum es geht

Früher gab es für den Arbeitsalltag „die eine Software“: ein großes Programm mit Kalender,
Aufgaben, Mail und Dashboard in Reitern. Mit einem Tiling-Window-Manager braucht es das nicht
mehr. Der Window Manager *ist* das Dashboard. TorroWorkplace liefert dafür die Bausteine:
**viele kleine, eigenständige Apps**, jede in ihrem eigenen Fenster, die sich frei über den
Desktop verteilen lassen.

- **Zuerst Hyprland (Omarchy)**, dann **AeroSpace auf dem Mac**, später Windows (komorebi/GlazeWM).
- **workspace-setups** merkt sich, welche Apps wo lagen und was in ihnen offen war, und stellt
  ganze Fenster-Sets per Tastendruck oder beim Start wieder her.
- **Ein Befehl installiert alles.**

## Warum gerade jetzt: Arbeiten mit KI

Wer mit mehreren KI-Agenten arbeitet (Claude Code, Codex, …), jongliert mit vielen gleichzeitig
laufenden Dingen: Sitzungen, Branches, Pull Requests, Termine, Aufgaben, Freigaben. Die Suite
soll das **übersichtlicher** machen, auf zwei Wegen:

1. **Für den Menschen:** Jede App beantwortet *eine* Frage auf einen Blick („Was steht heute an?“,
   „Welcher Agent wartet auf mich?“). Sie bleibt dafür sichtbar in ihrer Kachel liegen.
2. **Für die Agenten:** Jede App hat einen MCP-Server. Ein Agent kann eine Aufgabe anlegen,
   einen Termin nachschlagen oder einen Status melden. Mensch und Agent arbeiten auf denselben
   Daten, ohne Copy & Paste.

## Prinzipien

1. **Eine App, eine Frage, ein Fenster.** Lieber zwei kleine Apps als eine mit Reitern.
2. **Offene Standards zuerst.** CalDAV/iCalendar (Nextcloud, iCloud, …) vor proprietären APIs.
   Andere Anbieter kommen als zusätzliche Provider hinzu, nie als Umbau.
3. **Offline zuerst.** Lokaler Cache, Sync im Hintergrund. Die Oberfläche wartet nie auf das Netz.
4. **Dasselbe Gesicht nach außen.** Jede App hat: TUI, native GUI, `status --json`, `ctl`, `mcp`.
5. **Feste App-ID.** Jedes Fenster hat eine stabile Klasse (`torro-<app>`), damit Window-Manager-Regeln
   und workspace-setups es zuverlässig wiederfinden.
6. **Leicht.** Zehn Apps gleichzeitig dürfen den Rechner nicht spürbar belasten, deshalb kein Electron.
7. **Plattformübergreifend von Anfang an mitgedacht.** Rust-Kern, keine Linux-only-Abkürzungen im
   Kern. Plattformspezifisches liegt in klar getrennten Backends.
8. **Torro-Designsprache.** Tokens aus `torro-design`, Rot ist Marke und nie Status.
