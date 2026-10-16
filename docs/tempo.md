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

Gemessen mit `scripts/measure-start.sh --runs 3` (Median, ms seit Programmstart, erster Lauf verworfen).
„Erstes Bild“ ist der erste Aufbau der Ansicht (`view`), noch nicht das Zeichnen auf dem Schirm; „Laden“ die
Antwort des ersten Ladens.

**Vorher (TP.1, 2026-10-16), warm (Dienst läuft):**

| App | Debug: erstes Bild | Debug: erste Daten | Debug: Laden | Release: erstes Bild | Release: erste Daten | Release: Laden |
|---|---|---|---|---|---|---|
| Mail-Fenster | 168 | 303 | 191 | 88 | 150 | 104 |
| Mitteilungen | 170 | 315 | 187 | – | – | – |
| Kalender | 259 | 414 | 1 | 102 | 139 | 2 |
| Aufgaben | 225 | 360 | 2 | 82 | 126 | 1 |
| Assistent | 211 | 344 | – | 92 | 124 | – |
| Steuerzentrale | 183 | 949 | 917 | 104 | 580 | 548 |

**Vorher (TP.1), kalt (Dienst läuft nicht, die App startet ihn):**

| App | Debug: erstes Bild | Debug: erste Daten | Release: erstes Bild | Release: erste Daten |
|---|---|---|---|---|
| Mail-Fenster | 192 | 304 | 109 | 134 |
| Kalender | 206 | 735 | 183 | 335 |
| Aufgaben | 267 | 426 | 169 | 285 |
| Assistent | 204 | 314 | 136 | 189 |
| Steuerzentrale | 189 | 649 | 144 | 416 |

Dienst allein (`--core`) bis „socket task started“: Debug 41 ms, Release ~80 ms.

Befunde:
- Der Dienst ist schneller bereit (40–80 ms), als eine App bis zu ihrer ersten Anfrage braucht (~100–270 ms).
  Darum trifft die erste Anfrage beim Kaltstart meist schon den laufenden Dienst; der Fehler mit 10 s Warten
  tritt nur auf, wenn der Dienst langsamer startet (z. B. gesperrter Schlüsselbund nach der Anmeldung).
- Ein Kaltstart des Kalenders brauchte einmal 16,5 s bis zu den ersten Daten; in fünf weiteren Läufen nicht
  wiederholbar (320–375 ms).
- Von außen gemessen (Bildfolge per `screencapture`) stand die Mailliste im Debug-Build erst nach ~0,8 s auf dem
  Schirm, die Marke „erste Daten“ kommt nach 0,3 s: der Rest ist Zeichnen, im Debug-Build unoptimiert.
- Steuerzentrale: Ihr erstes Laden fragt 16 Requests nacheinander. `system.status` braucht 99 ms (Abfrage des
  Fenstermanagers), die übrigen bis auf einen unter 1 ms; den Rest der 550 ms braucht vermutlich
  `ai.list_models` für das Standardmodell, das den KI-Server fragt.
- Neuladen im Mail-Fenster (Debug, drei Timer-Runden): Laden 44–84 ms im Hintergrund, kein `update` oder
  `view` über 16 ms beim Neuladen. Drei langsame `update` (44–104 ms) kamen ohne Bezug zum Laden.

**Debug, Abhängigkeiten optimiert (TP.2, `[profile.dev.package."*"] opt-level = 2`), warm:**

| App | erstes Bild | erste Daten | Laden |
|---|---|---|---|
| Mail-Fenster | 95 | 157 | 88 |
| Kalender | 84 | 107 | 1 |
| Aufgaben | 95 | 119 | 1 |
| Assistent | 92 | 142 | – |
| Steuerzentrale | 108 | 768 | 735 |

Von außen gemessen steht das Mail-Fenster samt Liste jetzt nach ~0,18 s auf dem Schirm (vorher ~0,8 s). Ein
voller Debug-Build dauert einmal länger (hier knapp 2 Minuten), danach baut Cargo nur den eigenen Code neu.

Nachher: trägt TP.4 ein.
