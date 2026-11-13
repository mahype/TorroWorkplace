# Terminal-Werkzeug für den Assistenten

Stand: 2026-11-13, Issue #706. **Diskussionsgrundlage:** Hier ist nichts entschieden und nichts gebaut. Das Dokument
macht die Grundsatzfrage entscheidbar und endet mit einer Empfehlung und der Liste der Entscheidungen, die ein Mensch
treffen muss. Es gibt keinen Code, keinen Request und keinen Katalogeintrag; alle Namen unten sind Vorschläge.

Frage: Der Assistent soll am Ende so viel können wie ein Agent im Terminal: Befehle ausführen, Programme starten,
Ergebnisse lesen. Wie geht das mit den Rechten, Freigaben und Sperren der Suite zusammen?

## Ausgangslage

- **Entscheidung 6** in [assistent.md](assistent.md): „Dateien und Shell beim Agent-Antrieb immer aus.“ Ein
  Terminal-Werkzeug des Kerns ist keine Shell eines fremden Agenten, hebt aber ihren Zweck auf: Etwas
  auf dem Rechner führt Befehle auf Wunsch eines Modells aus. Entscheidung 6 selbst bleibt unverändert; sie meint die
  eingebauten Werkzeuge von Agenten wie Claude Code (so auch der Schluss von „Werkzeug Dateien“).
- **Entscheidung 9:** Heute schon möglich ist ein MCP-Server mit Shell-Werkzeugen unter „Your own servers“, mit 60
  Sekunden je Aufruf. Der Mensch stimmt dem Server und jedem Werkzeug zu; was der Server tut, prüft der Kern nicht.
  Ein eigenes Terminal-Werkzeug wäre dasselbe mit Karte, Ordnergrenzen und Protokoll, nicht mehr Macht.
- **„Grenze des Modells“** in [api.md](api.md): Freigaben schützen vor einem Agenten, der über API oder MCP spricht,
  nicht vor Code, der schon als derselbe Nutzer läuft. Ein solches Programm liest den App-Schlüssel neben dem Socket
  und ruft `approvals.decide_approval` selbst auf. **Ein Befehl aus dem Terminal-Werkzeug ist genau so ein Programm.**
  Ohne Sandbox kann er jede Freigabe der Suite umgehen, auch die, die ihn selbst erlaubt hat. Das ist der Kern der
  Frage: Eine Freigabe-Karte allein ist für dieses Werkzeug keine Sicherheitsgrenze, sondern eine Rückfrage an die
  Person. `api.md` sagt dort auch: „Eine Sandbox gegen Programme desselben Nutzers ist nicht Ziel.“ Stufe 2 weicht davon ab (nur
  für dieses Werkzeug); `api.md` ist dann anzupassen (Folge-Issues).
- **Hermes Agent** (Abgleich vom 2026-11-13, Quellcode der lokalen Installation): keine Sandbox in der Voreinstellung.
  Etwa 100 Muster erkennen gefährliche Befehle; Modi `manual`, `smart` (ein Hilfsmodell entscheidet) und `off`;
  Antworten „einmal“, „Sitzung“, „immer“, „ablehnen“; harte Grenzen, die nie umgangen werden; ohne Mensch wird
  abgelehnt. Das Projekt nennt das selbst eine Heuristik und keine Sicherheitsgrenze und ein lokales Terminal
  zusammen mit fremden Inhalten „außerhalb der unterstützten Sicherheitslage“. Wir übernehmen daraus die Antworten
  und die harten Grenzen, nicht den Modus `smart` und nicht „immer“ ohne Sandbox.
- **Seit #666** starten Assistent und Bots mit allen Werkzeugen. Ein Terminal darf in „alle“ nicht enthalten sein
  (Abschnitt „Rechte“).

## Zwei Wege

| | Weg A: Freigaben ohne Sandbox | Weg B: Sandbox je Plattform |
|---|---|---|
| Idee | Jeder Befehl wartet auf ein Ja der Person; der Befehl läuft mit ihren Rechten. | Der Befehl läuft in einer Hülle, die nur freigegebene Ordner und kein Netz (oder ein begrenztes) kennt. |
| Aufwand | gering: Karte, Prozess starten, Ausgabe lesen. Läuft auf allen drei Plattformen gleich. | hoch und je Plattform anders; Tests brauchen echte Betriebssysteme. |
| Schutz | nur so gut wie die Aufmerksamkeit der Person und der Text auf der Karte. Ein harmlos aussehender Befehl kann Schlimmes tun (`make`, `npm install`, ein Skript). | Der Kern setzt Grenzen, die ein eingeschleuster oder irrender Befehl nicht überschreitet, auch wenn die Person zustimmt. |
| Lücken | Der Befehl liest den App-Schlüssel und entscheidet Freigaben selbst. Zugriff auf alles, was die Person darf. Die Prüfung des Befehlstexts ist eine Heuristik. | Fehler in der Hülle; Programme, die in der Hülle nicht laufen (Entwicklerwerkzeuge brauchen oft Netz und viele Ordner); Aufwand, die Hülle aktuell zu halten. |

Weg B nach Plattform (Stand 2026-11; Fassungen und Rechte vor dem Bau prüfen):

| Plattform | Mittel | Aufwand und Stand |
|---|---|---|
| Linux | bubblewrap (eigener Namensraum, Ordner nur lesend oder schreibend eingebunden, Netz abschaltbar) oder Landlock mit seccomp (ohne Zusatzprogramm, Dateizugriff und Systemaufrufe begrenzt) | mittel. Landlock braucht einen neueren Kern und deckt je Fassung unterschiedlich viel ab; bubblewrap ist ein Fremdprogramm und fehlt auf manchen Rechnern. |
| macOS | Seatbelt (`sandbox-exec` mit eigenem Profil) | mittel, aber **`sandbox-exec` gilt als veraltet**; Apple bietet keine gleichwertige, öffentlich gedachte Schnittstelle für Befehlszeilenprogramme. Es kann ohne Vorwarnung wegfallen. |
| Windows | AppContainer (mit eingeschränktem Token) | hoch. Kein einfaches Gegenstück; Ordnerfreigaben und Netz sind je Programm fummelig. Bleibt vorerst bei Weg A. |

**Jede Sandbox sperrt** die Ordner der Suite (Einstellungen, Daten und Zustand jeder App), die App-Schlüssel (`app.key`
und die Schlüsseldateien der Clients) `core.sock` und den Session-Bus des Systems (D-Bus; über ihn meldet die Benachrichtigung „Allow“, siehe „Grenze des Modells“). Ohne diese Sperre wäre sie wirkungslos: Mit dem Schlüssel
spricht der Befehl wieder als die App (siehe „Ausgangslage“). Die Sperren sind dieselben wie bei „Immer gesperrt“ des
Werkzeugs Dateien (Abschnitt „Harte Grenzen“); in der Sandbox kommt als Mittel das Betriebssystem dazu, nicht ein
Textvergleich.

## Befehle

Namen als Vorschlag, Bereich `tools`, neues Werkzeug `terminal`:

| Befehl | Zweck |
|---|---|
| `tools.run_command` | führt einen Befehl aus und wartet auf sein Ende (mit Zeitgrenze); Antwort: Ausgabe, Exit-Code, `truncated` |
| `tools.start_command` | startet einen Befehl im Hintergrund, Antwort: Kennung |
| `tools.read_output` | liest neue Ausgabe eines Hintergrundbefehls ab einer Stelle, mit Zustand (läuft, beendet, Exit-Code) |
| `tools.stop_command` | beendet einen Hintergrundbefehl (erst höflich, nach kurzer Frist hart) |

- **Ohne Shell:** Das Modell liefert ein Programm und Argumente als Liste, keinen Text für eine Shell, wo es
  geht. Braucht es Pipes oder `&&`, ist das ein eigener, ausdrücklich gezeigter Fall („Run in a shell“) mit ungekürzter
  Karte; die Prüfung des Befehlstexts (Abschnitt „Harte Grenzen“) gilt dann für den ganzen Text.
- **Arbeitsordner:** nur ein Ordner aus „Allowed folders“ mit „Read and write“ (Entscheidung 27). Ohne Angabe fragt die
  Karte nach dem Ordner; einen Ordner mit „Read“ gibt es nicht als Arbeitsordner. Der Kern löst den Pfad auf wie das
  Werkzeug Dateien (`canonicalize`).
- **Grenzen** (Zahlen sind eine Entscheidung für Menschen, Vorschlag in Klammern): Zeit je Befehl (zum Beispiel 120
  Sekunden bei `run_command`, danach wird der Befehl beendet), Ausgabe je Aufruf (zum Beispiel 20.000 Zeichen; mehr
  schneidet der Kern hinten ab und sagt `truncated`), Ausgabe je Hintergrundbefehl im Speicher (Ring von zum Beispiel
  1 MB, ältere Teile fallen weg). Weder Zeit noch Ausgabe lassen sich vom Modell erhöhen.
- **Hintergrundprozesse:** höchstens wenige je Unterhaltung (zum Beispiel 3) und insgesamt (zum Beispiel 8). Sie enden mit
  dem Dienst und mit der Unterhaltung (Löschen, Abbrechen); ein Prozess, der sich abkoppelt, wird als Gruppe beendet
  (Prozessgruppe bzw. Job-Objekt unter Windows). Sie überleben keinen Neustart des Dienstes; `read_output` auf eine
  unbekannte Kennung ist `not-found`.
- **Eingabe von Hand** (Passwortfragen, interaktive Programme) gibt es nicht: Der Befehl bekommt kein Terminal und
  nichts auf der Standardeingabe. Ein Programm, das fragt, scheitert.

## Freigabe

- **Die Karte zeigt** den Befehl so, wie der Kern ihn ausführt: Programm und Argumente, entschärft (Steuerzeichen und
  umgekehrte Textrichtung sichtbar gemacht, Zeilenumbrüche sichtbar), **ungekürzt aufklappbar**; darüber der
  Arbeitsordner, die Zeit- und Ausgabegrenze und ob eine Sandbox gilt. Nie der Text des Modells, wenn er vom
  ausgeführten abweicht (wie bei den Freigabe-Karten des Werkzeugs Dateien).
- **Antworten:**
  - **Allow once:** genau dieser Aufruf.
  - **Allow in this conversation:** nur der **exakt gleiche** Befehl im selben Ordner, bis die Unterhaltung endet oder
    der Dienst neu startet (im Speicher, wie das Ja der Websuche). Kein „ähnlicher“ Befehl und keine Programmliste:
    Ein Muster wie „`git *`“ ließe `git -c core.sshCommand=…` laufen.
  - **Deny:** der Aufruf scheitert mit `forbidden`; das Modell bekommt keinen Grund über das Wort „abgelehnt“ hinaus.
- **„Immer“ gibt es erst in Stufe 2**, mit Sandbox (Abschnitt „Empfehlung“). Gespeichert wird es bei den Rechten des
  Clients (`clients.json`), je Eintrag Programm und Argumente und Ordner, nicht in der Unterhaltung. **Ändern können
  es nur Menschen** (Punkt 2 in „Was nie nach außen geht“): setzen, ändern und entfernen nur mit dem App-Schlüssel.
- **„Ask first: Never“ gilt nicht.** Wie beim Papierkorb des Werkzeugs Dateien fragt das Terminal immer einen Menschen,
  auch wenn der Client bei „Ask first“ „Never“ gewählt hat. Es fragt auch bei Lesebefehlen wie `ls`: Der Kern kann
  nicht wissen, was ein Programm tut.
- **Nie per Stimme.** Eine Freigabe für das Terminal entscheidet man nur mit einem Klick auf der Karte (die Aktion der
  Benachrichtigung zählt für das Terminal nicht, weil das D-Bus-Signal nicht nach Absender gefiltert wird), nie durch eine gesprochene Antwort: Ein Mikrofon hört auch Fernseher und
  Nachbarn, und ein Befehl ist kein Kalendereintrag.
- **Eine offene Frage** steht wie jede Freigabe in `approvals.json` (0600). Sie läuft nach der üblichen Frist ab; ein
  Befehl, der nach dem Ablauf kommt, braucht eine neue Karte. Das Protokoll (`audit.jsonl`) nennt Client, Befehl
  `tools.run_command` und Kennungen, **nie den Befehlstext** (wie immer ohne Inhalte); der Verlauf der Unterhaltung
  enthält ihn.

## Harte Grenzen

Aus Entscheidung 31 und „Immer gesperrt“ des Werkzeugs Dateien (Abschnitt „Werkzeug Dateien“ in
[assistent.md](assistent.md)). Diese Dinge bekommt das Terminal **nie**, auch nicht mit Ja auf der Karte:

- die Ordner der Suite (Einstellungen, Daten und Zustand jeder App), `app.key`, die Schlüsseldateien der Clients,
  `core.sock` bzw. die Pipe unter Windows;
- die Dateien mit Client-Schlüsseln und Hooks (`~/.claude.json`, `~/.claude/settings.json` … mit ihren Sicherungen)
  und das Programm, das die Hooks starten;
- die Schlüsselablagen des Systems (`~/.ssh`, `~/.gnupg`, Schlüsselbund), weitere bekannte Ablagen von Zugangsdaten
  (Cloud, GitHub, Paketregister, Passwortspeicher) und die Browser-Profile;
- der Papierkorb des Systems (endgültig Löschen dort ist kein Ausweg am Papierkorb-Befehl vorbei).

**Was das heißt, ohne Sandbox:** Ohne Sandbox kann der Kern diese Sperren **nicht durchsetzen**. Ein Programm liest
`~/.ssh/id_ed25519`, wenn es will. Möglich ist nur eine **Prüfung des Befehlstexts**: Der Kern lehnt einen Befehl ab,
dessen Argumente einen gesperrten Pfad nennen (aufgelöst, ohne Groß- und Kleinschreibung, auch `~` und
Umgebungsvariablen in der Schreibweise des Systems), und eine Liste von Mustern für offensichtlich gefährliche
Befehle (`rm -rf` auf Home und Wurzel, Formatieren, Schreiben auf Geräte, `curl … | sh`, `sudo`). **Das ist eine
Heuristik, keine Grenze:** Ein Skript, das den Pfad erst zusammensetzt, ein Interpreter mit `-c`, eine Datei, die
vorher geschrieben wurde, umgehen sie. Das Konzept benennt das ausdrücklich, in der Karte („No sandbox: Torro can't
stop this command from reading other files.“) und im Rechte-Editor, statt eine Sicherheit zu versprechen, die es nicht
gibt. Erst mit Sandbox (Stufe 2) sind die Sperren Sperren.

Zusätzlich gelten die Grenzen der Ordner: Arbeitsordner nur aus „Allowed folders“ mit „Read and write“; ein Befehl, der
Pfade außerhalb als Argument bekommt, ist ohne Sandbox ein Befund der Heuristik, mit Sandbox gar nicht erreichbar.

## Fremde Inhalte

**Eine Unterhaltung, die fremde Inhalte gelesen hat, führt keinen Befehl mehr aus.** Es gibt dann keine Rückfrage
(in der Planung entschieden).

- **Was als fremd zählt:** alles, was Abschnitt C.11 und `untrusted` im Kern
  (`crates/torroworkplace/src/assistant/tools.rs`) schon zählen: Mail, GitHub, Einträge des Eingangs und von Agenten,
  Antworten von Bots, Antworten fremder MCP-Server. Dazu **hier auch** Treffer der Websuche und gelesene Webseiten,
  die C.11 bei der Websuche selbst ausnimmt (damit nicht jede zweite Suche fragt): Beim Terminal wäre das Fehler
  umgekehrt, denn eine Webseite ist der beste Weg, einem Modell einen Befehl unterzuschieben.
- **Warum keine neue Freigabe wie bei der Websuche:** Auf der Karte steht ein Befehl, der harmlos aussieht und es
  nicht ist (`npm test` im Ordner, in dem ein eingeschleustes Skript liegt; `git status` mit einem manipulierten
  Konfigurationseintrag). Die Person sieht, was läuft, nicht warum. Eine Frage, die man mit Ja beantworten kann,
  verschiebt die Last auf die Person zu genau dem Zeitpunkt, an dem das Modell am wenigsten zu trauen ist.
- **Wie es sich anfühlt:** Hat die Unterhaltung fremde Inhalte gelesen, meldet das Werkzeug `forbidden` mit dem Grund
  „This conversation has read content from outside. Start a new conversation to run commands.“. Die Person kann eine
  neue Unterhaltung beginnen. Anders als das Ja der Websuche geht die Sperre beim Neustart nicht verloren: Der Kern
  bestimmt sie aus dem Verlauf neu, nach den Namen der Werkzeuge, die fremde Inhalte lesen können (die Zeile im
  Verlauf hält die Antwort nicht fest; es zählt, was gelesen worden sein kann).
- **Restgefahr:** Was ein Befehl **selbst** ausgibt (eine Datei, die er liest, eine Antwort aus dem Netz), ist
  ebenfalls fremd, sobald es ein Modell erreicht. Die Ausgabe eines Befehls zählt deshalb wie ein Treffer der Websuche:
  Nach einem Befehl, dessen Ausgabe Inhalte von außen enthalten kann (`curl`, `git log`, ein Test), ist die
  Unterhaltung nicht mehr sauber. Das Konzept lässt die Frage offen, ob jede Ausgabe so zählt (Entscheidung 4 unten);
  die strenge Antwort („ja“) macht das Werkzeug fast nutzlos für lange Aufgaben, die lockere lässt eine Lücke.

## Umgebung und Ausgabe

- **Umgebung des Prozesses** wie bei MCP-Servern: leer (`env_clear`), dann genau die Grundvariablen `BASICS`
  (`HOME`, `USER`, `LOGNAME`, `LANG`, `LC_ALL`, `TMPDIR`; unter Windows `WINDOWS_BASICS`) und der `PATH`, den der Kern
  setzt (`crates/torroworkplace/src/extensions/env.rs`). **Nie** `TORRO_*` und nie ein Schlüssel, den der Dienst
  zufällig hat. Namen, die verändern, welchen Code ein Programm lädt (`PYTHONPATH`, `NODE_OPTIONS`, `LD_PRELOAD`,
  `BASH_ENV` …), bleiben draußen: `blocked_env` (`crates/torro-ipc/src/extensions.rs`) ist die Grundlage. Das Modell
  gibt keine Umgebungsvariablen mit; wer welche braucht, setzt sie in der Karte (Entscheidung offen).
- **Ausgabe schwärzen:** Der Kern ersetzt Geheimnis-Muster in der Ausgabe (Zugangsdaten in Adressen, `Authorization:`,
  private Schlüsselblöcke, typische Token-Formen wie `ghp_…` oder `sk-…`, Zeilen der Form `NAME_KEY=…`, `…TOKEN=…`, `…SECRET=…`,
  `…PASSWORD=…`, wie sie ein Befehl aus einer Datei liest, etwa `.env`; im Prozess selbst gibt es solche Variablen nicht) durch `[redacted]`, **bevor** die Ausgabe zum Modell, in den Verlauf auf der Platte
  oder auf die Karte geht. **Das ist eine Heuristik:** Ein Geheimnis in einer ungewohnten Form kommt durch, und ein
  Befehl kann es verschlüsselt ausgeben. Es senkt das Risiko, es ist keine Zusage; die Oberfläche sagt das nicht auf
  jeder Karte, aber die Einstellungen des Werkzeugs nennen es.

## Bots, API, MCP und geplante Läufe

- **Bots** wie der Assistent: Ein Bot hat sein eigenes Terminal-Recht nur mit Haken (Abschnitt „Rechte“); im Auftrag
  des Hauptbots gilt die Schnittmenge beider Rechte, und die Freigabe läuft in der Unterhaltung des Hauptbots, wo die
  Person sie sieht. Fremde Inhalte aus beiden Läufen zählen (wie bei der Websuche).
- **Clients über API und MCP** haben **in Stufe 1 kein Terminal.** Sie haben keine Unterhaltung und deshalb keine
  Karte; eine Freigabe ohne die Karte fragt in `approvals` und erscheint nur als Benachrichtigung, ohne den Befehl
  im Zusammenhang zu zeigen. Ein Agent, der Befehle ausführen will, hat seine eigenen Werkzeuge; ein Client, der
  `assistant_send_message` darf, lässt den Assistenten mit dessen Rechten arbeiten (Abschnitt „Drei Wege“ in
  [assistent.md](assistent.md)); der Haken liegt dort nie bei einem fremden Agenten. **Technisch:** `terminal` ist in der Auswahl von Clients über API
  und MCP nicht wählbar (der Editor zeigt es nicht an); ein Request von dort endet mit `forbidden`, auch wenn eine
  Rechte-Datei den Eintrag trüge.
- **Geplante Läufe** (Erinnerungen, Zeitpläne, spätere Hintergrundaufgaben) bekommen das Terminal **nie.** Es gibt
  keinen Menschen, der die Karte sieht; „ohne Mensch wird abgelehnt“ gilt wie bei Hermes. Das ist keine Entscheidung
  für später, sondern eine Voraussetzung: Wer geplante Läufe baut, nimmt das Terminal aus deren Werkzeugen heraus.
- **Browser-Steuerung und Code-Ausführung** (Rechnen in einer Hülle ohne Dateien und Netz) sind eigene Werkzeuge mit
  eigenen Fragen. Hier nur eingeordnet: Browser-Steuerung greift auf angemeldete Sitzungen zu und gehört in die
  Gruppe „braucht einen Haken, nie in alle“; Code-Ausführung in einer Hülle ohne Dateien, Netz und Schlüssel ist
  weniger gefährlich als ein Terminal und könnte ohne Karte laufen. Beide folgen nach diesem Konzept, nicht in ihm.

## Rechte

- **Neue Klasse „nie in alle“.** `ToolId::needs_naming` (`crates/torro-core/src/settings.rs`) gilt heute für `web`: „Alle
  Werkzeuge“ deckt es für Schlüssel von API- und MCP-Clients nicht ab. Das reicht für das Terminal **nicht**, weil
  `Rights::reached_tools` (`crates/torro-ipc/src/api.rs`) nach `Rights::opened` auch `web` gibt: Assistent und Bots
  starten offen, und „alle“ erreicht dort jedes Werkzeug, das `needs_naming` hat (Entscheidung 26 in
  [assistent.md](assistent.md)). Das Terminal braucht deshalb eine **strengere** Klasse: nie in „alle“, auch nicht
  nach `opened`, nicht in einer Schnittmenge, nicht in einer Vorlage (auch nicht *Full access*). Es gibt es
  nur, wenn die Auswahl des Clients `terminal` ausdrücklich nennt.
- **Der Haken** setzt nur ein Mensch (App-Schlüssel, Punkt 2 in „Was nie nach außen geht“). Ein Agent kann ihn
  weder setzen noch erweitern; auch **zurücknehmen** geht nur mit dem App-Schlüssel, denn der Haken liegt in der Auswahl des Clients
  (`clients.update_client`, `clients.*` ist nur mit dem App-Schlüssel erreichbar). Anders als beim Mikrofon und bei den
  Websuche-Anbietern (`settings.set_setting`) gibt es keinen Weg für Agenten, nicht einmal zum Abschalten; das ist
  strenger als nötig, aber ohne neue Logik. Ob ein eigener Abschalt-Weg sinnvoll ist, ist Entscheidung 11.
- **Warnung im Rechte-Editor** (Assistent › Einstellungen › „API and MCP“ › „Used by the assistant“): neben dem
  Schalter „Terminal“ steht, was er heißt: „Lets the assistant run programs on this computer as you. Torro can't
  fully limit what they do.“, und ob eine Sandbox gilt. Der Schalter steht beim Anlegen eines Bots aus.
- **Bestehende Clients** ändern sich nicht: Wer das Terminal nie bekommen hat, bekommt es auch nach einem Update nicht
  (wie `web` für Schlüssel nach #305).

## Drei Wege

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Befehl ausführen | Chat: Werkzeugzeile und Freigabe-Karte | `tools.run_command` · Write, Freigabe immer | `tools_run_command` (Stufe 1: nur durch den Assistenten, nicht für fremde Clients) |
| Hintergrundbefehl starten | ebenso | `tools.start_command` · Write, Freigabe immer | `tools_start_command` (ebenso) |
| Ausgabe lesen | Werkzeugzeile | `tools.read_output` · Read, nur für Befehle des eigenen Clients | `tools_read_output` |
| Hintergrundbefehl beenden | Werkzeugzeile, „Stop“ auf der Zeile | `tools.stop_command` · Write | `tools_stop_command` |
| Befehl freigeben („Allow once“, „in this conversation“, „Deny“) | Karte im Chat | `approvals.decide_approval` (gibt es) | **Never** (Punkt 3) |
| Haken „Terminal“ für einen Client setzen | Assistent › „Used by the assistant“; Steuerzentrale › API clients, MCP clients | `clients.update_client` · Admin | **Never** (Punkt 2) |
| „Immer erlaubt“-Liste ändern (Stufe 2) | Assistent › „Used by the assistant“ | `clients.update_client` · Admin | **Never** (Punkt 2) |
| Grenzen (Zeit, Ausgabe, Hintergrundbefehle) | – | – (in Stufe 1 feste Werte des Kerns; einstellbar erst später, Entscheidung 2) | – |

`Never` ist nur, was in „Was nie nach außen geht“ in [api.md](api.md) steht: den Haken setzen und die „Immer“-Liste
ändern (Punkt 2: kein Agent ändert Rechte) und die Freigabe entscheiden (Punkt 3). Alles andere hat ein MCP-Tool. Ein
fremder Client sieht `tools_run_command` in Stufe 1 trotzdem nicht, weil sein Recht auf `terminal` fehlt.

## Empfehlung

**Ein Stufenweg** (in der Planung entschieden, mit der Annahme des Plans bestätigt):

1. **Stufe 1 – Karte bei jedem Befehl, ohne Sandbox.** Jeder Befehl fragt. Erlaubt sind „einmal“ und „für diese
   Unterhaltung“ (nur der exakt gleiche Befehl). **Kein „Immer“.** Kein Terminal für fremde Clients, kein Terminal
   in geplanten Läufen. Fremde Inhalte sperren das Werkzeug für die Unterhaltung. Es gibt eine Prüfung des
   Befehlstexts, aber sie heißt überall Heuristik. Alle drei Plattformen. **Der Nutzen ist klein und ehrlich:** Der
   Assistent kann die Aufgaben einer Person erledigen, die ihm dabei zusieht.
2. **Stufe 2 – Sandbox unter Linux und macOS.** Erst dann „Immer“ für genau einen Befehl samt Ordner, gespeichert bei
   den Rechten, nur durch Menschen zu ändern. Die Sandbox sperrt die Ordner der Suite, die Schlüssel und den Socket.
   **Windows bleibt bei Stufe 1**, bis es einen tragfähigen Weg gibt.

**Verworfen:**

- **„Sandbox zuerst“:** Es dauert lange bis zum ersten Nutzen, und unter Windows gibt es ihn gar nicht. Stufe 1 ist
  nützlich und ehrlich, wenn sie ihre Grenze benennt.
- **„Kein eigenes Terminal“:** Wer Befehle ausführen will, nimmt dann weiter einen MCP-Server mit Shell (Entscheidung
  9) ohne Karte, ohne Ordnergrenzen, ohne Sperre für fremde Inhalte. Das ist schlechter. Und das Ziel des Assistenten
  („so viel können wie ein Agent im Terminal“) würde verfehlt.
- **Modus `smart` (ein Hilfsmodell entscheidet):** Ein zweites Modell, das fremde Inhalte lesen kann, wäre derselben
  Einschleusung ausgesetzt wie das erste.
- **Eine neue Freigabe nach fremden Inhalten (wie bei der Websuche):** siehe „Fremde Inhalte“.

## Entscheidungen für Menschen

Diese Fragen kann nur ein Mensch beantworten; der Plan lässt sie offen.

1. **Stufenweg gegenprüfen?** (Die Annahme des Plans hat ihn bestätigt; das hier ist die Gegenprobe vor dem Bau.) Stufe 1 ohne Sandbox gegen die Aussage in „Grenze des Modells“: ein Terminal ist ein
   Programm als derselbe Nutzer. Ist „Karte bei jedem Befehl“ ein ausreichender Schutz, wenn die Oberfläche die Grenze
   offen sagt? Alternative: erst Stufe 2.
2. **Zeit- und Ausgabegrenzen und Hintergrundprozesse:** Zahlen für Zeit je Befehl, Ausgabe je Aufruf, Ausgabe je
   Hintergrundbefehl, Anzahl je Unterhaltung und insgesamt (Vorschläge im Abschnitt „Befehle“).
3. **Windows:** bleibt es bei Stufe 1, oder wird Windows ohne Terminal ausgeliefert, bis eine Sandbox da ist?
4. **Ausgabe eines Befehls als fremd:** Zählt die Ausgabe jedes Befehls als fremder Inhalt (streng: danach keine
   Befehle mehr in der Unterhaltung), nur die von Programmen, die erkennbar von außen lesen (`curl`, `git fetch`),
   oder keine?
5. **Schwärzen:** Welche Muster, und ob die Oberfläche das Schwärzen sichtbar macht („Some values were hidden“)?
6. **Harte Grenzen ohne Sandbox:** Prüfung des Befehlstexts als Heuristik ja oder nein (sie kann falsche Sicherheit
   vortäuschen)? Wenn ja: nur gesperrte Pfade oder auch die Muster für gefährliche Befehle?
7. **Umgebungsvariablen:** Darf die Karte welche mitgeben (etwa für einen Test), oder nie?
8. **Eingabe und Terminal:** Gibt es nie ein Terminal für interaktive Programme (so in diesem Konzept) oder später
   einen eigenen Fall mit Karte?
9. **Shell erlauben:** Bleibt „Run in a shell“ ein ausdrücklicher Fall mit ungekürzter Karte, oder gibt es nur
   Programm und Argumente?
10. **Bots:** Dürfen Bots das Terminal überhaupt bekommen, oder bleibt es dem Assistenten vorbehalten?
11. **Abschalt-Weg für Agenten:** Darf ein Agent den Haken „Terminal“ zurücknehmen (neue Logik in `kept_for_people`), oder
    bleibt alles beim App-Schlüssel?

## Folge-Issues

Hier nur benannt, **nicht angelegt**; sie entstehen nach den Antworten auf die Entscheidungen:

- Stufe 1: Werkzeug `terminal` (Requests und Katalog, Karte, Grenzen, Umgebung, Schwärzen, fremde Inhalte)
- Sandbox Linux (bubblewrap oder Landlock)
- Sandbox macOS (Seatbelt, mit Plan für den Wegfall von `sandbox-exec`)
- `docs/api.md` anpassen: Der Satz „Eine Sandbox gegen Programme desselben Nutzers ist nicht Ziel“ in „Grenze des Modells“ gilt
  mit Stufe 2 nicht mehr für das Terminal
- Rechte-Klasse „nie in alle“ (`ToolId`, `Rights::reached_tools`, Rechte-Editor, Warnung)
