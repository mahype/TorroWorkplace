# Terminal-Werkzeug für den Assistenten

Stand: 2026-11-15, Issue #754 (Konzept aus #706). Der Assistent führt auf dem Rechner Befehle und Skripte aus und
liest ihre Ausgabe, so wie ein Agent im Terminal. Wie oft er nachfragt, wählt der Mensch. Die Freigaben folgen dem
Modell von Hermes Agent (entschieden am 14. November 2026): normale Befehle laufen ohne Nachfrage, gefragt wird bei
Befehlen, die auf eine Liste gefährlicher Muster passen. **Es gibt keine Sandbox.** Jede Prüfung des Befehlstexts und
das Schwärzen von Geheimnissen sind Faustregeln; die Oberfläche sagt das offen.

Code: `crates/torroworkplace/src/tools/terminal/` (Prüfung, Muster, Prozesse, Schwärzen), das Tor im Kern
(`Core::terminal_gate` in `core.rs`), die Karte im Chat (`crates/torroassistant/src/view.rs`), die Seite in der
Steuerzentrale (`crates/torroworkplace/src/ui/connections.rs`).

## Ausgangslage

- **Grenze des Modells** in [api.md](api.md): Freigaben schützen vor einem Agenten, der über API oder MCP spricht,
  nicht vor Code, der als derselbe Nutzer läuft. **Ein Befehl aus dem Terminal ist so ein Programm.** Er könnte den
  App-Schlüssel lesen und Freigaben selbst entscheiden. Die Karte ist deshalb eine Rückfrage an die Person, keine
  Sicherheitsgrenze.
- **Entscheidung 6** in [assistent.md](assistent.md) („Dateien und Shell beim Agent-Antrieb immer aus“) bleibt: Ein
  Agent (Antrieb B) bekommt das Terminal nicht, nur der Assistent mit seinem eigenen Modell.
- **Entscheidung 9:** Ein MCP-Server mit Shell-Werkzeugen bleibt möglich; das Terminal ist dasselbe mit Karte,
  Ordnergrenzen, Sperre nach fremden Inhalten und Protokoll.
- **Hermes Agent** (`tools/approval.py`, `tools/approval_detection.py`, MIT-Lizenz, Copyright Nous Research): Von dort
  kommen die beiden Musterlisten (`HARDLINE_PATTERNS`, `DANGEROUS_PATTERNS`), die Vorbereitung des Befehlstexts und die
  Antworten „einmal“, „in dieser Unterhaltung“, „immer“, „ablehnen“. Nicht übernommen: der Modus `smart` (ein
  Hilfsmodell entscheidet), der Modus `off` und die Regeln, die nur Hermes selbst schützen.

## Befehle

Bereich `tools`, ohne eigenes `ToolId`: Das Terminal steht außerhalb der Rechte (Abschnitt „Rechte“).

| Befehl | Zweck |
|---|---|
| `tools.run_command` | führt einen Befehl aus und wartet auf sein Ende; Antwort `command-output`: Ausgabe, Exit-Code, `truncated`, `timed_out`, `redacted` |
| `tools.start_command` | startet einen Befehl im Hintergrund; Antwort `command-started` mit Kennung |
| `tools.read_command_output` | liest die Ausgabe eines Hintergrundbefehls ab `from`; Antwort `command-status`: läuft, Exit-Code, Ausgabe, `next` |
| `tools.stop_command` | beendet einen Hintergrundbefehl, erst höflich (Unix: `SIGTERM` an die Gruppe), nach 3 s hart |

- **Programm oder Shell:** Das Modell gibt `program` mit `args` (ohne Shell, bevorzugt) oder `shell` (eine Zeile für
  `sh -c`, unter Windows `cmd /C`) nur für Pipes und `&&`. Genau eins von beiden, sonst `error`. Geprüft wird bei
  `program` der zusammengesetzte Text (Wörter mit Leerzeichen in einfachen Anführungszeichen), bei `shell` die ganze
  Zeile.
- **Arbeitsordner:** nur ein Ordner aus „Allowed folders“ mit „Read and write“, aufgelöst wie beim Werkzeug Dateien
  (`folders.rs`, `Allowed::existing`); außerhalb `not-found`.
- **Grenzen** (Entscheidung 2 des Issues): 120 s je `run_command`, danach wird der Befehl samt allem, was er
  gestartet hat, beendet (`timed_out`); 20.000 Zeichen Ausgabe je Antwort, mehr schneidet der Kern hinten ab
  (`truncated`); 1 MiB Ausgabe je Hintergrundbefehl im Speicher, ältere Teile fallen weg; höchstens 3
  Hintergrundbefehle je Unterhaltung und 8 insgesamt (`busy`). Weder Zeit noch Ausgabe lassen sich vom Modell ändern.
- **Hintergrundbefehle** gehören ihrer Unterhaltung: Eine fremde oder unbekannte Kennung ist `not-found`. Sie enden,
  wenn die Unterhaltung gelöscht oder ihre Antwort abgebrochen wird, wenn ein Mensch das Terminal ausschaltet, und mit
  dem Dienst. Stoppen geht immer, auch nach fremden Inhalten. Jeder Befehl läuft als
  Prozessgruppe (unter Windows im Job-Objekt ohne Konsolenfenster) wie die MCP-Server, damit nichts übrig bleibt.
- **Keine Eingabe** (Entscheidung 8): Die Standardeingabe ist leer, es gibt kein Terminal. Ein Programm, das fragt,
  bekommt nichts.
- **Windows** bekommt das Terminal wie Linux und macOS (Entscheidung 3).

## Zwei Modi

Nur ein Mensch schaltet um (Steuerzentrale › MCP servers › Built-in › Terminal).

- **„Bei jedem Befehl fragen“** (Voreinstellung, `every`): Vor jedem Befehl erscheint die Karte, außer genau dieser
  Befehl im selben Ordner ist erlaubt.
- **„Nur bei gefährlichen Befehlen fragen“** (`dangerous`): Ein Befehl läuft sofort, außer er passt auf ein Muster
  aus Hermes' Liste (`rm -rf`, `git reset --hard`, `curl … | sh`, Schreiben in Systemordner …) und nicht jedes dieser
  Muster ist erlaubt.

Im Rechte-Modus der Antwort „Ask“ fragt jeder Befehl, im Modus „Read only“ laufen nur `read_command_output`. Gefragt
wird auch bei einem Client mit „Ask first: Never“.

## Freigabe

- **Die Karte im Chat** zeigt den Ordner, den ganzen Befehl entschärft (Steuerzeichen und unsichtbare Zeichen
  sichtbar gemacht) – lange Befehle gekürzt mit „Show all“ –, die Muster, die er trifft („Looks dangerous: recursive
  delete“), die Grenzen und „No sandbox: Torro can't stop this command from reading or changing other files.“. Die
  Zeilen baut der Kern aus dem geprüften Befehl, nie aus dem Text des Modells (`approvals::terminal_question`).
- **Antworten:** „Allow once“, „In this conversation“, „Always“, „Deny“. „In this conversation“ gilt im Speicher, bis
  die Unterhaltung endet oder der Dienst neu startet; „Always“ steht in den Einstellungen
  (`tools.terminal.always`, höchstens 200 Einträge).
- **Woran „Immer“ hängt** (in der Planung entschieden, 15. November 2026): im Modus „Nur bei gefährlichen“ wie bei
  Hermes am erkannten Muster – ein „Always“ für „recursive delete“ lässt danach jeden rekursiven Löschbefehl ohne Karte
  laufen, auch in anderen erlaubten Ordnern; im Modus „Bei jedem Befehl“ am genauen Befehl im selben Ordner.
- **Nur auf der Karte:** Ein „Allow“ ohne Reichweite (`approvals.decide_approval` ohne `scope`) ist für einen Befehl
  des Terminals `forbidden`. Die Benachrichtigung sagt nur „The assistant is waiting for your approval in the chat.“
  ohne Knopf, der Eingang legt keinen Eintrag an, die Freigaben-Seite der Steuerzentrale zeigt „Decide in the
  assistant's chat“ und nur „Deny“. Ablehnen geht überall.
- **Freigaben nie per Stimme**, Befehle nie in geplanten Läufen: Dort gibt es keine Karte. In einer Unterhaltung per
  Stimme hat der Assistent das Terminal wie im Text; was nach dem Modus fragt, wartet auf die Karte im Chat.
- **Nach dem Ja** prüft der Kern noch einmal: Terminal an, Client der Assistent, Befehl und Ordner wie gezeigt
  (sonst `conflict`), Ordner und harte Grenzen wie jetzt.
- **Protokoll:** `audit.jsonl` nennt den Befehlsnamen (`tools.run_command`) und bei Hintergrundbefehlen die Kennung,
  **nie den Befehlstext, den Ordner oder die Ausgabe**. Der Verlauf der Unterhaltung enthält den Befehl.

## Harte Grenzen

Diese Befehle laufen nie, auch nicht mit Ja (`forbidden` ohne Karte):

- **Gesperrte Pfade** im Befehlstext: die Ordner der Suite, die Dateien mit Client-Schlüsseln und Hooks, die
  Schlüsselablagen des Systems (`Tools::excluded_now`, wie beim Werkzeug Dateien). Jedes Wort des Befehls wird mit
  `~`, `$HOME` und `%USERPROFILE%` aufgelöst, relative Pfade ab dem Arbeitsordner, `.` und `..` ausgerechnet, ohne
  Groß- und Kleinschreibung verglichen.
- **Hermes' `HARDLINE_PATTERNS`:** rekursives Löschen von Wurzel, Systemordnern und Home, Formatieren, `dd` und
  Umleiten auf Blockgeräte, Fork-Bombe, alle Prozesse beenden, Herunterfahren und Neustarten.
- **Eigene Ergänzungen** (aus der API-Prüfung von #754): Programme, die die Schlüsselablagen des Systems lesen
  (`secret-tool`, `security find-…-password`, `kwallet-query`, `cmdkey /list`, `pass`, `op`, `bw`, `keyring get`):
  ihre Ausgabe wäre ein Geheimnis beim Modell. Und die Suite selbst: `torro…`-Programme mit `ctl` oder `mcp`,
  `core.sock`, `app.key`, die Pipe unter Windows; sonst entschiede ein Befehl Freigaben und Einstellungen, die nur
  Menschen gehören.

**Ohne Sandbox ist das eine Faustregel:** Ein Skript, das den Pfad erst zusammensetzt, ein Interpreter mit `-c` oder
eine vorher geschriebene Datei umgehen sie. Ein `cat` einer erlaubten Datei ist kein Befund.

## Fremde Inhalte

**Eine Unterhaltung, die fremde Inhalte gelesen hat, führt keinen Befehl mehr aus**, in beiden Modi und ohne
Rückfrage: „This conversation has read content from outside. Start a new conversation to run commands.“

- **Was zählt:** alles, was die Werkzeuge `untrusted` markieren (Mail, GitHub, Eingang, Bots, fremde MCP-Server), jedes
  frühere Werkzeug, das solche Inhalte gelesen haben kann (`may_have_read_untrusted`, hier mit Websuche und Webseiten
  und dem Lesen von Dateien über das Werkzeug Dateiinhalte), die Nachricht eines Clients und ein früherer Befehl, der
  erkennbar von außen liest. Der Verlauf zählt mit, auch nach einem Neustart.
- **Ausgabe eines Befehls** (Entscheidung 4): Sie zählt nur dann als fremder Inhalt, wenn das Programm erkennbar von
  außen liest (`curl`, `wget`, `git fetch|pull|clone`, `ssh`, `scp`, `rsync` zu einem Host, `gh api`, Paket
  installieren; `patterns::reads_outside`). Dann trägt die Antwort `untrusted: true`, und die Unterhaltung führt
  danach keinen Befehl mehr aus. `cat` einer Datei, `ls` oder `cargo test` machen die Unterhaltung nicht fremd.
- **Restgefahr:** Eine Datei, die ein Befehl liest, kann Text von außen enthalten, ohne dass der Kern es weiß.

## Umgebung und Ausgabe

- **Umgebung** wie bei MCP-Servern: leer (`env_clear`), dann die Grundvariablen `BASICS` (unter Windows auch
  `WINDOWS_BASICS`) und der `PATH`, den der Kern setzt. Nie `TORRO_*` und nie ein Schlüssel des Dienstes. Das Modell
  gibt keine Umgebungsvariablen mit (Entscheidung 7).
- **Schwärzen** (Entscheidung 5, `redact.rs`): Zugangsdaten in Adressen, `Authorization`- und Schlüssel-Header, private
  Schlüsselblöcke, Token-Formen (`ghp_…`, `github_pat_…`, `sk-…`, `xox…-`, `AKIA…`, `glpat-…`, `AIza…`) und Zeilen wie
  `API_KEY=…` oder `password: …` werden zu `[redacted]`, **bevor** die Ausgabe zum Modell, in den Verlauf oder auf eine
  Karte geht. Die Antwort sagt `redacted`, die Werkzeugzeile „some values hidden“. Eine ungewohnte Form kommt durch.

## Rechte

- **Kein Haken in den Rechten** (in der Planung entschieden): Nur der Assistent kann das Terminal bekommen; ein Mensch
  schaltet es auf der Seite „Terminal“ der Steuerzentrale ein. Rechte-Editor, `my_access` und Vorlagen bleiben
  unberührt; die Befehle haben kein `ToolId`.
- **Wer es erreicht:** nur der Client des Assistenten aus seiner eigenen Unterhaltung (`Host::run_terminal`, nur für
  `Actor::Assistant`). **Bots** bekommen es nicht (Entscheidung 10), auch nicht im Auftrag des Assistenten, ebenso
  wenig ein Agent mit geliehenem Schlüssel, ein Client über API oder MCP (`forbidden`, auch mit allen Rechten) oder der
  App-Schlüssel (`unsupported`). Das Modell bekommt die vier Werkzeuge nur angeboten, solange das Terminal an ist.
- **Schalter, Modus und „Immer“-Liste** ändert nur ein Mensch (Entscheidung 11, Punkt 15 in „Was nie nach außen geht“
  in [api.md](api.md)): `settings.set_setting terminal` und `terminal_always` nur mit dem App-Schlüssel, auch zum
  Abschalten; `settings.update_settings` eines Clients lässt sie, wie sie sind; `settings.get_settings` zeigt einem
  Client die „Immer“-Liste nicht.

## Drei Wege

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Befehl ausführen | Chat: Werkzeugzeile und Karte | `tools.run_command` · Delete | `tools_run_command` (für fremde Clients immer abgelehnt) |
| Hintergrundbefehl starten | ebenso | `tools.start_command` · Delete | `tools_start_command` (ebenso) |
| Ausgabe lesen | Werkzeugzeile | `tools.read_command_output` · Read | `tools_read_command_output` (ebenso) |
| Hintergrundbefehl beenden | Werkzeugzeile | `tools.stop_command` · Write | `tools_stop_command` (ebenso) |
| Befehl freigeben | Karte im Chat | `approvals.decide_approval` mit `scope` | **Never** (Punkt 3) |
| Schalter und Modus | Steuerzentrale › MCP servers › Built-in › Terminal | `settings.set_setting terminal` | **Never** (Punkt 15) |
| „Immer“-Eintrag entfernen | ebenda, × je Eintrag | `settings.set_setting terminal_always` | **Never** (Punkt 15) |

## Später

- Sandbox unter Linux (bubblewrap oder Landlock) und macOS (Seatbelt) als zusätzlicher Schutz; dann wären die harten
  Grenzen Sperren statt Faustregeln.
- Computer-Use (Bildschirm sehen und bedienen): eigenes Issue #755.
