# Tempo: Start und Laden der Fenster

Wie schnell die Fenster der Suite stehen und Daten zeigen, woran wir das messen und welche Werte gelten.
Aufgaben: [tasks/tempo.md](tasks/tempo.md). Issue: #206.

## Zielwerte

Gemessen auf macOS (Apple Silicon), Zeit ab dem Start des Programms. „Erste Daten“ heißt: die erste Liste
(Mail, Einträge, Termine, Aufgaben) ist gezeichnet. Nicht gezählt wird der erste Start nach einem neuen Build:
macOS prüft ein frisch gebautes Programm einmal (gemessen 1,4–1,7 s), danach nicht mehr.

| Fall | Build | Fenster sichtbar | Erste Daten |
|---|---|---|---|
| Dienst läuft | Release | < 0,3 s | < 0,5 s |
| Dienst läuft | Debug | < 0,5 s | < 1,0 s |
| Dienst läuft nicht (Kaltstart) | Release | < 0,5 s | < 1,5 s |
| Dienst läuft nicht (Kaltstart) | Debug | < 0,5 s | < 3,0 s |

Neuladen (Timer, Knopf „Refresh“): kein `update` und kein `view` dauert im Release-Build länger als 50 ms.
Die Daten holt der Dienst aus dem lokalen Speicher; kein Fenster wartet auf IMAP oder CalDAV.

## Messen

- `TORRO_TIMING=1` schreibt Zeitmarken auf stderr: Start, erstes Bild, Dienst antwortet, erste Daten, jedes
  Neuladen mit Dauer, jedes `update` und `view` über 16 ms (`torro_core::timing`).
- `scripts/measure-start.sh` startet eine App mehrmals, warm und kalt, und gibt die Marken als Tabelle aus.
  Kalt beendet es vorher den laufenden Dienst (`torroworkplace ctl quit`).
- Debug und Release getrennt messen; beim Release-Build den ersten Start nach dem Bauen verwerfen.

## Messwerte

### Stand Planung (2026-10-16, vor den Änderungen)

Mail-Fenster (`torroinbox --view mail`), Dienst läuft (Debug), 567 offene Einträge, ein Mail-Konto. Gemessen von
außen (Fenster in der Fensterliste, Bildfolge per `screencapture`).

| Build | Fenster sichtbar | Liste gezeichnet |
|---|---|---|
| Debug | 0,29–0,39 s | ~0,8 s |
| Release | 0,15–0,21 s | ~0,15 s |

Antwortzeiten des Dienstes (Debug-Build, je Request eine Verbindung):

| Request | Zeit | Größe |
|---|---|---|
| `system.ping` | 0,1 ms | 44 B |
| `mail.search_messages` (Ordner, 50) | 1 ms | 15 KB |
| `mail.get_message` (mit HTML) | 1–140 ms | bis 75 KB |
| `mail.list_folders` | 30 ms | 5 KB |
| `inbox.list_items` (alle offenen) | 34 ms | 566 KB |

Kaltstart: Läuft der Dienst nicht, startet ihn die App, fragt aber sofort und bekommt keine Antwort. Das
Mail-Fenster zeigt dann einen Fehler und fragt erst mit dem nächsten Timer wieder (10 s), Kalender und Aufgaben
nach 2 s, der Assistent nach 5 s.

### Vorher und nachher

Tragen die Aufgaben TP.1 (vorher) und TP.4 (nachher) ein.
