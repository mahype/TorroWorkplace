# Assistent: mit der Suite chatten

Stand: 2026-10-13, Issue #78. **Diskussionsgrundlage.** Wunsch: Die KI-Agenten, die man in der Steuerzentrale
anlegt, sollen sich auch in einer eigenen App benutzen lassen, als Chatfenster. Der Chat kennt die
Suite (Kontext als Skill), benutzt den eigenen MCP-Server bzw. dieselben Befehle und darf sofort auf
alle Bereiche zugreifen: Mail, Kalender, Aufgaben, Eingang, Workspaces, Konten, Einstellungen.

Baut auf [api.md](api.md) (Befehle, Rechte, Freigaben) und [ki.md](ki.md) (KI-Konten) auf.

## Leitbild: ein Jarvis für den Rechner

Am Ende ist der Assistent die **Zentrale**, die man einfach anspricht (Weckwort, siehe „Sprache“)
und über die man alles auf dem Rechner steuert, **soweit man es ihm freigibt**: heute Mail, Kalender, Aufgaben, Eingang, Workspaces und Fenster, später Dateien,
Programme und alles, was Erweiterungen dazubringen ([erweiterungen.md](erweiterungen.md)). Jede neue
Fähigkeit kommt als Befehl im Katalog dazu und wird damit automatisch ein Werkzeug des Assistenten
und ein Schalter in seinen Rechten. Es gibt keinen zweiten Weg am Katalog vorbei.

Dafür gilt: **Die Person entscheidet, was er sieht und darf**, Bereich für Bereich, und kann es
jederzeit ändern. Mehr Fähigkeiten heißt nie automatisch mehr Zugriff.

## Die Frage, die die App beantwortet

**„Kannst du das für mich erledigen?“** – in Sätzen statt in Klicks: „Was steht morgen an, und
welche Mails dazu sind noch offen?“, „Leg mir für jede Mail von Anna eine Aufgabe an“, „Lade die
Vorlage Schreiben und blockier mir den Nachmittag im Kalender“.

## Woher das Modell kommt: Open First

**Grundsatz: offen und lokal zuerst.** Der Assistent läuft ohne Abo eines großen Anbieters und
ohne Cloud, wenn man das will. Die erste Wahl sind **offene Modelle**: lokal über **Ollama** (auch
LM Studio, llama.cpp, vLLM) oder, wenn der Rechner zu klein ist, über **OpenRouter**, das offene
Modelle (Qwen, Llama, Mistral, DeepSeek, GLM …) über eine Schnittstelle und einen Schlüssel anbietet.
Geschlossene Modelle und fertige Agenten sind möglich, stehen aber hinten.

Der Assistent hat dafür zwei **Antriebe**; Oberfläche, Befehle und Rechte sind dieselben.
**Antrieb B wird seit Issue #219 nicht mehr angeboten** (Aufgaben C.7 und C.18): Er war nie
eingerichtet, sondern nur gefunden — es gab kein Konto, keinen Schlüssel und keinen Eintrag, also
sah niemand, warum dort Claude antwortet, und niemand konnte ihn abschalten. Claude läuft seit
KI.1b–KI.1d als KI-Konto über das Anthropic-Protokoll, mit eigenem Schlüssel wie jeder andere
Anbieter. Codex war als Antrieb ohnehin nie möglich (keine Positivliste für Werkzeuge). Der Code
bleibt liegen; die Spalte unten beschreibt, was er tut, falls er je zurückkommt.

| | **A: KI-Konto (eigener Antrieb)** – zuerst | **B: Agent als Antrieb** – danach |
|---|---|---|
| Was läuft | der Kern ruft das Modell direkt über ein KI-Konto ([ki.md](ki.md)) | ein verbundener Agent ohne Terminal: `claude -p --input-format stream-json --output-format stream-json`, `codex exec --json`, Gemini CLI |
| Anbieter, in dieser Reihenfolge | 1. **Ollama** und andere lokale Server (OpenAI-kompatibel, ohne Schlüssel) · 2. **OpenRouter** (OpenAI-kompatibel, ein Schlüssel, Filter „open weights“) · 3. eigener Server · 4. Mistral, Anthropic, OpenAI, Google | Claude Code, Codex, Gemini CLI |
| Werkzeuge | der Kern gibt dem Modell die Befehle aus dem Katalog als Werkzeuge (Function Calling) und führt sie selbst aus | der Agent spricht mit `torroworkplace mcp`, wie heute |
| Kontext | Skill als Systemprompt | Skill als zusätzlicher Systemprompt (`--append-system-prompt`) |
| Daten | lokal bleiben lokal; Datenregel je Quelle aus [ki.md](ki.md) gilt (Mail standardmäßig nur an lokale Modelle, Schalter `mail_to_cloud` aus C.17) | gehen an den Anbieter des Agenten; Mail nur mit dem Schalter `mail_to_cloud` (C.17) |
| Voraussetzung | KI.1 aus [ki.md](ki.md) (KI-Konten im Kern) | Agent installiert und verbunden |

**Vorschlag:** C beginnt mit **A**: KI.1 wird dafür vorgezogen, zuerst mit dem OpenAI-kompatiblen
Protokoll, das Ollama, LM Studio, vLLM und OpenRouter gemeinsam sprechen. Anthropic-Protokoll und
Antrieb **B** folgen. Im Assistenten wählt man je Unterhaltung Konto und Modell; Standard ist der
zuletzt benutzte Antrieb, beim ersten Start ein gefundenes lokales Modell.

Was offene Modelle brauchen, damit das gut geht:

- **Werkzeug-fähige Modelle:** Der Kern fragt bei Ollama (`/api/show`, `capabilities: tools`) und
  OpenRouter (`supported_parameters` enthält `tools`) nach und bietet nur solche Modelle an.
  Empfehlung in der Auswahl für den Rechner, z. B. Qwen 3 (8B/14B/32B), Llama 3.3, Mistral Small.
- **Wenige Werkzeuge auf einmal:** Kleine Modelle verlieren bei 80 Werkzeugen den Überblick. Der Kern
  gibt nur die Werkzeuge der Bereiche, die der Assistent darf und die in der Unterhaltung an sind,
  und bei kleinen Modellen zunächst nur die lesenden. Ist die Zahl zu groß, bekommt das Modell ein
  Werkzeug `find_tools` (Suche im Katalog), das die passenden nachlädt.
- **Kurzer Kontext:** Ergebnisse von Werkzeugen werden gekürzt (Mail-Text abgeschnitten, Listen mit
  Anzahl), die volle Fassung bleibt aufklappbar in der App.
- **Ehrlich bei Grenzen:** Kann ein lokales Modell keine Werkzeuge, sagt die Auswahl das, statt es
  still schlecht zu machen.

Bei **B** läuft der Agent mit einer eigenen MCP-Konfiguration nur für den Assistenten
(`--mcp-config` mit `--strict-mcp-config`), ohne die übrigen MCP-Server und ohne Shell und
Dateizugriff (`--allowedTools` nur `mcp__torroworkplace__*`), damit der Chat die Suite bedient und
nicht nebenbei das System. Dateien und Shell bleiben **immer aus** (entschieden); wer sie braucht,
nutzt den Agenten im Terminal.

## Rechte: freigeschaltet, was die Person will

Der Assistent ist **ein eigener Client** „Assistant“ der Art `assistant`, gleich welcher Antrieb ihn
gerade antreibt (entschieden); im Protokoll steht der Antrieb dazu. Er ist nicht der Client von
Claude Code im Terminal: So sieht man, was der Chat getan hat, und gibt ihm andere Rechte als dem
Agenten im Terminal.

- **Beim ersten Start** fragt die App, was der Assistent sehen und tun darf: je Bereich ein Schalter
  mit Stufe (Kein Zugriff · Lesen · Lesen und Schreiben · Voll) und den Sammlungen (welche
  Mail-Konten, Kalender, Listen, Quellen des Eingangs, Repositories). Vorlagen wie bei jedem Client
  (*Read only*, *Read and write*, *Full access*); vorausgewählt ist nur *Read only* für
  Kalender, Aufgaben und Eingang.
- **Ändern jederzeit** im Assistent-Fenster › Einstellungen › Access (seit #231, vorher in der
  Steuerzentrale), derselbe Rechte-Editor wie für die Clients unter API und MCP, und aus dem Chat heraus über
  einen Hinweis „The assistant has no access to Mail. Change…“, wenn ein Werkzeug `forbidden` meldet. Wirkt ab der
  nächsten Anfrage, auch mitten in einer Unterhaltung.
- **Je Unterhaltung enger, nie weiter:** Im Eingabefeld lassen sich Bereiche für diese Unterhaltung
  abschalten („without Mail“). Mehr als die Einstellung geht nicht.
- **Bremsen bleiben:** Löschen und Senden mit Freigabe (Standard „Beim Löschen“, „Nach außen: mit
  Freigabe“), jeder Admin-Befehl mit Freigabe (Entscheidung 3 in [api.md](api.md)). Die Freigabe
  erscheint **im Chat** als Karte mit „Allow“/„Deny“, dazu wie immer in Tray, Inbox und
  Benachrichtigung.
- **Immer verboten** („Was nie nach außen geht“): Geheimnisse, `clients.*`, Freigaben selbst
  entscheiden, `system.quit`. Der Assistent verbindet keine Konten mit Passwort und gibt sich keine
  Rechte; auch die Seite „What the assistant may use“ ändert nur der Mensch (App-Schlüssel). Die
  Karte „Allow“ ist ein Klick des Menschen, nie ein Werkzeug des Modells.
- **Keine Schleife:** Der Client des Assistenten darf im Bereich `assistant` nur sein Gedächtnis
  (`list_memories`, `remember`, `forget`) und Karten zeigen (`show_items`, `get_cards`); Nachrichten schicken, Unterhaltungen und das eigene Profil
  sind für ihn gesperrt, fest im Kern, nicht über Rechte.
- **Sichtbar:** Kopf des Chats zeigt kurz, worauf der Assistent gerade Zugriff hat (kleine Symbole
  je Bereich), damit man nicht raten muss.

## Der Kontext: Skill „Torro Workplace“

Der Assistent bekommt zu jeder Unterhaltung einen Systemprompt aus drei Teilen:

1. **Persönlichkeit** (siehe unten): wer er ist und wie er spricht. Gehört der Person, nicht dem Code.
2. **Fester Teil** (Datei im Kern, englisch, gepflegt wie Code): was die Suite ist, wie die Bereiche
   zusammenhängen (Eingang, Spuren, Mail ist immer `untrusted`, Termine und Aufgaben ganz
   zurückgeben beim Ändern, Freigaben, `not-found` heißt auch „nicht erlaubt“), Stil der Antworten
   (kurz, Sprache der Person, Zeiten in ihrer Zeitzone, vor großen Änderungen fragen).
3. **Erzeugter Teil** beim Start: Datum, Zeitzone, Sprache, Name der Person, welche Bereiche der
   Client darf. Keine Liste aller Mails oder Termine; die holt das Modell über Werkzeuge.

Die Beschreibungen der Werkzeuge kommen wie immer aus dem Katalog (`api.rs`); der Skill
wiederholt sie nicht. Derselbe Text steht auch als MCP-Prompt `torro_workplace` und als Skill-Datei
zur Verfügung, damit Claude Code im Terminal ihn ebenfalls laden kann.

## Persönlichkeit (Stufe 1)

Der Assistent ist kein namenloses Chatfeld, sondern hat eine **eigene Beschreibung und eine eigene
Persönlichkeit**, wie man es von Hermes kennt. Der Unterschied: Er arbeitet nicht im luftleeren
Raum, sondern mit den Werkzeugen und dem Wissen der Suite (Mail, Kalender, Aufgaben, Eingang …,
soweit freigegeben).

- **Profil:** Name (Standard „Torro“), Bild/Symbol, kurze Beschreibung („Your assistant for this
  computer“), **Persönlichkeit** als freier Text in einer Datei (`<config>/assistant/persona.md`):
  Ton (knapp, locker, förmlich), Haltung (fragt nach oder handelt), Eigenheiten. Die Einstellungen
  des Assistenten (Zahnrad › Profile) bieten ein paar Vorlagen als Start, danach frei änderbar.
- **Weckwort und Stimme** gehören zum Profil („Hey Torro“ passt zum Namen).
- **Grenzen der Persönlichkeit:** Sie ändert Ton und Verhalten, nie Rechte oder Regeln. Der feste
  Teil des Prompts (Mail ist `untrusted`, vor großen Änderungen fragen, Freigaben) steht **nach**
  der Persönlichkeit und gewinnt bei Widersprüchen.
- **Gedächtnis** (entschieden, Stufe 1): Notizen, die der Assistent sich auf Wunsch merkt
  („Anna ist meine Kollegin aus dem Vertrieb“), als lesbare Datei `<config>/assistant/memory.md`,
  in den Einstellungen des Assistenten (Zahnrad › Profile › Memory) einsehbar, änderbar und
  löschbar. Nichts wird heimlich gemerkt.
  Hat die Unterhaltung fremde Daten gelesen (Mail, GitHub, Einträge von Agenten und Skripten), braucht jede
  neue Notiz eine Freigabe, damit kein Text von außen unbemerkt in den Prompt gelangt; Notizen, die ein lokales
  Modell nach dem Lesen von Mail anlegt, bleiben „Nur auf diesem Computer“ (C.10, C.11).

## Bots (Stufe 2)

Nach dem Vorbild der Bots bei Grok und der GPTs bei OpenAI, bei uns einfach **Bots**: weitere
Agenten mit eigenem Profil, die auf ein Fachgebiet zugeschnitten sind, z. B. „Mail-Sortierer“,
„Reiseplaner“, „Release-Helfer“ oder „Wochenrückblick“.

- **Ein Bot hat:** Name, Bild, Beschreibung, Persönlichkeit, Anweisungen, eigenes Wissen (Skills,
  später Dateien), Antrieb und Modell (Open First wie oben) und **eigene Rechte** (eigener Client
  der Art `bot`).
- **Zwei Wege zu jedem Bot:**
  1. **Direkt:** Man wählt ihn in der App (Seitenleiste „Bots“) oder per Weckwort/Namen („Hey
     Torro, frag den Reiseplaner …“) und chattet mit ihm wie mit dem Hauptbot.
  2. **Über den Hauptbot, nur auf Ansage** (entschieden): Sagt die Person „frag den Reiseplaner …“,
     gibt der Hauptbot die Aufgabe ab (`bots.ask_bot`). Von sich aus darf er einen passenden Bot
     nur vorschlagen. Der Kern setzt das durch: `bots.ask_bot` des Hauptbots geht nur, wenn die
     letzte Nachricht der Person den Bot nennt oder sie den Vorschlag im Chat bestätigt. Die Antwort des Bots erscheint im Chat des
     Hauptbots als aufklappbarer Schritt („Asked Travel planner · 3 steps“).
- **Rechte beim Abgeben:** Arbeitet ein Bot im Auftrag des Hauptbots, gilt die **Schnittmenge** der
  Rechte beider. So bekommt der Hauptbot über einen Bot nie mehr, als er selbst darf, und der Bot
  nie mehr als seine eigenen Rechte.
- **Keine Ketten:** Ein Bot kann keine weiteren Bots fragen (Tiefe 1), damit nichts im Kreis läuft
  und man jeden Schritt nachvollziehen kann.
- **Anlegen:** in der App des Assistenten. Der Hauptbot kann einen Bot **vorschlagen**
  (Name, Beschreibung, Persönlichkeit, Anweisungen), ein neuer Bot hat aber keine Rechte, bis der
  Mensch sie vergibt (Punkt 2 in „Was nie nach außen geht“). Wer einen Bot selbst anlegt, vergibt
  die Rechte gleich im Bot-Editor (Abschnitt „Access“: Stufe je Bereich und „Ask first“, B.5);
  welche Kalender, Konten und Repositories, Senden und Admin-Befehle stellt man im Assistent-Fenster
  unter Einstellungen › Access bei diesem Bot ein.
- **Später:** Bots als Paket weitergeben (nur Daten: Profil, Anweisungen, Skills, verlangte
  Rechte), passend zu den Erweiterungen ([erweiterungen.md](erweiterungen.md), Punkt 5).

## Die App `torroassistant`

- **Fenster:** Seitenleiste mit Unterhaltungen (neueste oben, Suche), rechts der Verlauf, unten das
  Eingabefeld mit Antrieb-Auswahl. Mini-Größe: nur Eingabe und letzte Antwort (für eine Kachel).
- **Werkzeug-Aufrufe** erscheinen zusammengeklappt als eine Zeile („Searched mail · 12 results“,
  „Created event · Tue 14:00“), aufklappbar mit Eingabe und Ergebnis. Was geändert wurde, ist
  anklickbar und öffnet die passende App (Termin im Kalender, Eintrag in der Inbox).
- **Freigaben** als Karte im Verlauf mit der Zusammenfassung des Kerns (nie dem Text des Modells),
  „Allow“/„Deny“. Dieselbe Freigabe steht auch in Tray, Inbox und Benachrichtigung.
- **Stop** bricht eine laufende Antwort ab.
- Texte über `tr()`; Design nach `torro-design`.

### Eigene Chatfenster

Wie beim Schreiben einer Mail lässt sich eine Unterhaltung aus dem Hauptfenster **als eigenes
Fenster lösen**: nur Verlauf und Eingabefeld, ohne Seitenleiste. So steht der Chat als Kachel neben
dem Kalender oder der Inbox, und mehrere Chats können gleichzeitig offen sein.

- **Ein Programm, zwei Fensterarten** (wie bei `torroinbox`): `torroassistant` ist das
  Hauptfenster (Fensterklasse `torro-assistant`), `torroassistant --chat <id>` ein einzelnes
  Chatfenster (Fensterklasse `torro-assistant-chat`). `--chat new` öffnet eine neue Unterhaltung,
  in Stufe 2 `--bot <id>` den Chat mit einem Bot (neueste Unterhaltung mit ihm oder eine neue).
- **Öffnen:** „Open in new window“ im Kontextmenü einer Unterhaltung, Doppelklick, Ziehen aus der
  Seitenleiste; aus Inbox, Kalender und Aufgaben über „Ask about this“ direkt als Chatfenster.
- **Workspace-Vorlagen** merken die Chatfenster mit ihrem Argument; beim Laden kommt dieselbe
  Unterhaltung bzw. derselbe Bot wieder. Ist die Unterhaltung inzwischen gelöscht, öffnet das
  Fenster eine neue mit demselben Antrieb bzw. Bot.
- **Ein Zustand, viele Fenster:** Verlauf und laufende Antworten liegen im Kern. Ist dieselbe
  Unterhaltung im Hauptfenster und als eigenes Fenster offen, zeigen beide dasselbe.
- **Fenstertitel:** Name der Unterhaltung, in Stufe 2 mit Name des Bots, damit Kachel-Regeln und
  Fensterlisten sie unterscheiden.

Später: Tastenkürzel „Ask Torro…“ (Schnellfenster), Eintrag im Tray, „Ask about this“ aus Inbox,
Kalender und Aufgaben mit dem Eintrag als Kontext.

## Karten im Chat

Stand: 2026-10-14. Findet oder ändert der Assistent einen Termin, eine Aufgabe oder eine Mail,
erscheint im Verlauf nicht nur ein Satz, sondern eine **Karte**: der Eintrag in seiner eigenen
Gestalt (Kalenderblatt, Karteikarte, Mail-Vorschau), mit Knöpfen zum Weitermachen. Entwurf:
[`docs/prototypes/assistant/cards.html`](prototypes/assistant/cards.html).

### Grundsätze

- **Die Karte verweist, sie kopiert nicht.** Im Verlauf steht nur, *welcher* Eintrag gemeint ist
  (neuer Eintrag `EntryContent::Card` mit Art, Konto bzw. Sammlung und Kennung). Was die Karte zeigt,
  holt der Kern beim Anzeigen frisch. So bleibt sie aktuell, wenn der Termin später verschoben wird,
  und das Modell kann nichts hineinschreiben. Wie bei Freigaben gilt: Was auf der Karte steht, kommt
  vom Kern, nie aus dem Text des Modells.
- **Mit den Rechten des Assistenten aufgelöst.** Die App darf mehr sehen als der Assistent. Darum löst
  der Kern Karten mit den Rechten des Clients „Assistant“ auf, nicht mit denen der App. Ist ein Eintrag
  gelöscht oder inzwischen gesperrt, zeigt die Karte „No longer available“.
- **Knöpfe sind Klicks des Menschen.** Jeder Knopf schickt einen normalen `torro_ipc::Request` der
  App, nie ein Werkzeug des Modells. Fast alle gibt es schon (`calendar.save_event`,
  `tasks.save_todo`, `mail.respond_to_invitation` …).
- **Mail bleibt `untrusted`.** Karten mit Text aus Mails tragen die Kennzeichnung sichtbar.
- **Jede Karte hat eine Kurzform** von einer Zeile für Mini-Kachel und Schnellfenster.

### Wie Karten entstehen (entschieden)

Auf zwei Wegen:

1. **Automatisch nach schreibenden Werkzeugen.** Hat der Assistent einen Termin angelegt oder
   geändert, eine Aufgabe gespeichert oder erledigt, einen Entwurf gespeichert, hängt der Kern je
   geändertem Eintrag eine Karte an. Das gilt unabhängig davon, wie gut das Modell ist.
2. **Auf Anweisung bei Suchen.** Bei Ergebnissen von Suchen und Listen wählt das Modell mit dem neuen
   Werkzeug `assistant.show_items`, was als Karte erscheint, damit nicht 40 Termine im Chat stehen.
   Der Kern lässt nur Kennungen zu, die in einem Werkzeug-Ergebnis **dieser Unterhaltung** vorkamen,
   und höchstens 10 je Aufruf. So kann das Modell weder etwas erfinden noch an Daten vorbei, die es
   nie gelesen hat. `assistant.show_items` kommt zu den wenigen Befehlen im Bereich `assistant`, die der
   Assistent selbst darf (neben dem Gedächtnis, siehe „Keine Schleife“).

### Senden und Löschen auf der Karte (entschieden)

- **Die Entwurfs-Karte ist die Freigabe.** Will der Assistent eine Mail senden (`mail.send_draft`),
  entsteht wie bisher eine Freigabe. Im Chat erscheint dafür keine allgemeine Freigabe-Karte, sondern
  die volle Vorschau der Mail (Von, An, Betreff, Text, Anhänge) mit **Send** (= Allow) und
  **Discard** (= Deny), dazu „Edit in mail window“. In Tray, Inbox und Benachrichtigung steht dieselbe
  Freigabe wie immer.
- **Ohne offene Freigabe** (der Assistent hat nur einen Entwurf gespeichert) trägt die Karte ebenfalls
  „Send“. Der Klick sendet direkt, mit den Rechten der App: Es ist dieselbe Entscheidung des Menschen,
  nur ohne vorherige Anfrage des Modells.
- **Gesendet wird, was man sieht.** Die Vorschau kommt aus dem gespeicherten Entwurf. Die Freigabe bzw.
  der Knopf gilt für genau diesen Stand; ändert sich der Entwurf danach, wird „Send“ ungültig und die
  Karte zeigt den neuen Stand. Ein „Undo send“ gibt es vorerst nicht.
- **Löschen genauso:** Wartet eine Lösch-Freigabe für einen Termin oder eine Aufgabe, zeigt deren
  Karte **Delete** (= Allow) und **Keep** (= Deny) statt einer allgemeinen Freigabe-Karte.
- Das Modell selbst sendet und löscht weiter nur mit Freigabe; per Stimme lässt sich keine erteilen.

### Die ersten Karten

| Karte | Zeigt | Knöpfe | Neu nötig |
|---|---|---|---|
| **Termin** | Kalenderblatt (Monat, Tag, Wochentag), Titel, Zeit, Ort, Teilnehmer, Kalender als Farbe; Überschneidungen immer benannt (siehe unten) | Open in calendar · Send by mail · Edit · Delete; bei Einladungen Accept/Decline; Join bei Videolink | Termin als Anhang eines Entwurfs: `DraftAttachment::Event`, der Kern erzeugt die `.ics` |
| **Aufgabe** | Haken, Titel, Fälligkeit (überfällig in Attention-Orange), Liste, Priorität, Links zu Mail oder Issue | Done · Later (morgen, nächste Woche) · Open in tasks · Block time | — |
| **Mail-Entwurf** | Von, An, Cc, Betreff, Text, Anhänge (z. B. die `.ics`) | Send · Edit in mail window · Discard | — |

**Überschneidungen werden immer benannt.** Kollidiert ein Termin mit einem anderen, zeigt die Karte
den anderen Termin mit Namen und Zeit („Overlaps with Team sync 14:30–15:00“, Warnsymbol in
Attention-Orange), bei mehreren alle bzw. die ersten zwei und „+ 2 more“. Der Kern berechnet das aus
den Kalendern, die der Assistent sehen darf; das Modell muss nicht daran denken. Legt der Assistent
einen Termin an, verschiebt ihn oder nimmt eine Einladung an, die kollidiert, nennt er die
Überschneidung zusätzlich in seiner Antwort (fester Teil des Skills).

„Send by mail“ am Termin legt einen Entwurf mit dem Termin als Anhang an und zeigt ihn als
Entwurfs-Karte darunter: Man sieht immer, was rausgeht, bevor es rausgeht.

### Issue-Entwurf

Konzept: „Issues anlegen“ in [github.md](github.md). Sagt die Person „Mach daraus ein Issue“, ruft der
Assistent `tasks.create_issue` (mit `from`: die Unterhaltung oder der gefundene Eintrag). Weil Issues
nach außen gehen, wartet der Befehl auf Freigabe, und wie beim Mail-Entwurf **ist die Karte die
Freigabe**:

- **Zeigt:** Repo (`owner/repo`, mit „public“, wenn öffentlich, und dem Hinweis „Everyone can read
  the issue.“, wenn der Text aus Mail oder Eingang stammt), Titel, Labels, Zuständige, Meilenstein,
  Text als Markdown, die Zeile mit dem Weg zurück; darunter „Possibly the same as #41“ mit Link, wenn
  der Kern ähnliche offene Issues findet; Schalter „Also add to tasks“ (Standard aus).
- **Knöpfe:** **Create issue** (= Allow) · **Edit** (Felder auf der Karte; danach schickt die App einen
  neuen Request mit ihrem Schlüssel und lehnt die alte Freigabe ab; das Protokoll verknüpft beide, damit
  der Anstoß des Assistenten erkennbar bleibt) · **Discard** (= Deny); bei
  Treffern zusätzlich „Add comment there instead“.
- **Danach** wird sie zur Issue-Karte: `owner/repo#142`, Zustand offen, „Open on GitHub“, „Add to
  tasks“, und bei einer Mail als Quelle „Reply with link“ (legt einen Mail-Entwurf an, der als
  Entwurfs-Karte erscheint).

Später, nach denselben Regeln: Tagesplan, Zeitvorschläge („Wann habe ich Zeit?“, braucht einen
Freebusy-Befehl), gefundene Mail, Inbox-Eintrag, Workspace-Vorlage mit Skizze der Kacheln,
GitHub-Issue/PR, Konto-Status, Bot-Vorschlag, „Gemerkt“, Änderung mit Rückgängig, Rückfrage mit
Auswahl, Briefing, Person.

## Sprache: per Weckwort mit dem Assistenten reden

Ziel: Man sagt ein **Weckwort** (zum Start fest „Hey Torro“, später frei wählbar) und spricht einfach los: „Was steht heute
noch an?“, „Such mir die Mail von Anna mit dem Angebot raus.“ Der Assistent antwortet gesprochen und
zeigt das Ergebnis in einem kleinen Fenster (Schnellfenster mit Antwort und Treffern zum Anklicken).

Open First gilt auch hier, und das Mikrofon verlässt den Rechner nie, solange niemand das Weckwort sagt:

| Baustein | Lokal (Standard) | Hinweis |
|---|---|---|
| **Weckwort** | openWakeWord (offen, eigenes Wort trainierbar) | läuft dauerhaft, nur auf dem Rechner, kleiner Speicher (wenige Sekunden Ringpuffer, nie auf Platte) |
| **Sprache → Text** | whisper.cpp bzw. faster-whisper (Modell wählbar nach Rechner) | optional über ein KI-Konto, wenn die Person es erlaubt |
| **Text → Sprache** | Piper (offene Stimmen, auch Deutsch) | abschaltbar, dann nur Fenster |

- **Standard aus.** Einschalten nur in den Einstellungen des Assistenten (Zahnrad › Voice), mit
  Wahl des Mikrofons und Test.
- **Immer sichtbar:** Solange das Weckwort-Lauschen an ist, zeigt das Tray-Symbol das; während der
  Aufnahme deutlich (Punkt im Tray, Ton beim Start und Ende). Ein Kürzel schaltet sofort stumm.
- **Kein Lauschen durch Agenten:** Mikrofon an- oder ausschalten ist ein Schalter des Menschen. Kein
  Agent und kein Modell kann es einschalten (entschieden: Punkt 6 in „Was nie nach außen geht“,
  [api.md](api.md)).
- **Gleiche Rechte, gleiche Bremsen:** Gesprochenes landet als normale Nachricht in einer
  Unterhaltung („Voice“), mit denselben Rechten und Freigaben; eine Freigabe lässt sich per Stimme
  nicht erteilen, nur per Klick (sonst könnte ein Video im Hintergrund „Allow“ sagen).
- **Technisch:** Das Lauschen läuft als eigener Prozess `torroassistant --listen` (Mikrofon und
  Modelle getrennt vom Dienst), erkannter Text geht als `assistant.send_message` an den Kern.

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Sprache an/aus, Weckwort, Mikrofon, Stimme | Assistent › Einstellungen › Voice, Tray | `settings.set_setting` · Admin | **Never** für das Einschalten des Mikrofons (Punkt 6); Stimme und Weckwort über `settings_set_setting` |
| Gesprochene Nachricht | Schnellfenster | `assistant.send_message` (gibt es dann) | ebenso |

## Wo die Logik liegt

Wie überall im Kern (CLAUDE.md): Die App schickt Requests, der Kern führt die Unterhaltung. Der Kern
ruft das Modell (A) bzw. startet den Agenten (B), führt Werkzeuge aus, prüft Rechte, legt Freigaben
an und speichert den Verlauf. Darum kann man dieselbe Unterhaltung auch über API und MCP führen.

- **Speicher:** `<data>/assistant/<id>.jsonl` (0600), je Unterhaltung Nachrichten und
  Werkzeug-Aufrufe. Inhalte von Mails stehen damit auf der Platte wie im Mail-Cache; „Delete
  conversation“ löscht die Datei. Unterhaltungen werden nach **30 Tagen** gelöscht (entschieden,
  einstellbar bis „forever“); angeheftete bleiben.
- **Protokoll** (`audit.jsonl`) wie immer ohne Inhalte: Client `assistant`, Befehl, Kennungen.
- **Laufende Antwort:** Die App fragt `assistant.get_conversation` mit `after` (letzte gesehene
  Stelle) ab, bis es Änderungsmeldungen (`subscribe`) gibt.

## Drei Wege

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Antriebe auflisten (installiert, verbunden, Modell) | Auswahl im Eingabefeld | `assistant.list_engines` · Read | `assistant_list_engines` |
| Unterhaltungen auflisten | Seitenleiste | `assistant.list_conversations` · Read | `assistant_list_conversations` |
| Unterhaltung lesen (ab Stelle) | Verlauf | `assistant.get_conversation` · Read | `assistant_get_conversation` |
| Nachricht schicken (neue oder bestehende Unterhaltung) | Eingabefeld | `assistant.send_message` · Write (startet einen Modelllauf) | `assistant_send_message` |
| Antwort abbrechen | „Stop“ | `assistant.cancel` · Write | `assistant_cancel` |
| Unterhaltung umbenennen | Kontextmenü | `assistant.rename_conversation` · Write | `assistant_rename_conversation` |
| Unterhaltung als eigenes Fenster öffnen | „Open in new window“, „Ask about this“ | `assistant.open_window { conversation \| new \| bot }` · Write (startet `torroassistant --chat …`) | `assistant_open_window` |
| Unterhaltung anheften (bleibt über die Aufbewahrung hinaus) | Kontextmenü | `assistant.pin_conversation` · Write | `assistant_pin_conversation` |
| Unterhaltung löschen | Kontextmenü | `assistant.delete_conversation` · Delete | `assistant_delete_conversation` |
| Standard-Antrieb, Aufbewahrung | Assistent › Einstellungen › General bzw. History | `settings.set_setting` · Admin (gibt es) | `settings_set_setting` |
| Rechte des Assistenten | Assistent › Einstellungen › Access (seit #231), erster Start („Turn on assistant“) | `clients.update_client` (gibt es) | **Never** (Punkt 2: kein Agent ändert Rechte) |
| Bereiche je Unterhaltung abschalten | Eingabefeld | Feld `without` in `assistant.send_message` | ebenso |
| Profil und Persönlichkeit lesen | Assistent › Einstellungen › Profile | `assistant.get_profile` · Read | `assistant_get_profile` |
| Profil und Persönlichkeit ändern | ebenda | `assistant.update_profile` · Admin (mit Freigabe) | `assistant_update_profile` |
| Gedächtnis lesen / ergänzen / löschen | ebenda › Memory; im Chat „Remember …“ | `assistant.list_memories` · Read, `assistant.remember` · Write, `assistant.forget` · Delete | `assistant_list_memories`, `assistant_remember`, `assistant_forget` |
| Eigene Rechte kennen (Issue #134) | im Chat „Welche Rechte hast du?“ (Prompt-Zusammenfassung); die Person sieht sie in Assistent › Einstellungen › Access | `assistant.my_access` · Read (Assistent und Bots durch ihre Art, andere Clients mit `assistant`) | `assistant_my_access` |
| **Stufe 2:** Bots auflisten | Seitenleiste „Bots“ | `bots.list_bots` · Read | `bots_list_bots` |
| **Stufe 2:** Bot anlegen/ändern (ohne Rechte) | Bot-Editor | `bots.save_bot` · Write | `bots_save_bot` |
| **Stufe 2:** Bot löschen | Bot-Editor | `bots.delete_bot` · Delete | `bots_delete_bot` |
| **Stufe 2:** Bot fragen | Chat mit dem Bot; Hauptbot gibt ab | `bots.ask_bot` · Write (startet einen Modelllauf) | `bots_ask_bot` |
| **Stufe 2:** Rechte eines Bots | Bot-Editor › Access (Stufen je Bereich, „Ask first“, B.5); alles Feinere in Assistent › Einstellungen › Access bei diesem Bot | `clients.list_clients` · Admin, `clients.update_client` · Admin (gibt es, nur App-Schlüssel) | **Never** (Punkt 2) |
| Freigabe im Chat entscheiden | Karte „Allow“/„Deny“, auf Entwurfs- und Eintragskarten „Send“/„Discard“ bzw. „Delete“/„Keep“ | `approvals.decide_approval` (gibt es) | **Never** (Punkt 3) |
| Einträge als Karten zeigen | Karten im Verlauf | `assistant.show_items` · Write (nur Kennungen aus dieser Unterhaltung, max. 10) | `assistant_show_items` |
| Karten einer Unterhaltung auflösen | Karten im Verlauf | `assistant.get_cards` · Read (mit den Rechten des Assistenten) | `assistant_get_cards` |
| Termin als `.ics` an einen Entwurf hängen | „Send by mail“ auf der Termin-Karte | `mail.save_draft` mit `DraftAttachment::Event` · Write | `mail_save_draft` |

Neue Bereiche `assistant` und (Stufe 2) `bots` im Katalog. Ein Bot (Client `bot`) darf im Bereich
`bots` nichts (keine Ketten); der Hauptbot darf `bots.list_bots` und `bots.ask_bot`. Ein fremder Agent, der `assistant_send_message` darf, lässt
damit den Assistenten mit **dessen** Rechten arbeiten. Darum ist der Bereich `assistant` für andere
Clients standardmäßig „Kein Zugriff“, auch in der Vorlage *Full access* (entschieden); man schaltet ihn je Agent bewusst an.

## Reihenfolge

Aufgaben: [tasks/assistent.md](tasks/assistent.md) (C.1–C.9). Antrieb A braucht KI.1 (wird dafür
vorgezogen, OpenAI-kompatibel zuerst), Antrieb B nur einen verbundenen Agenten.
Sprache (C.9) kommt zuletzt, wenn der Chat steht. Persönlichkeit und Gedächtnis sind Teil von Stufe 1
(C.10), Bots sind Stufe 2 (Aufgaben B.1–B.5, geplant am 2026-10-14).

## Offen

Nichts. Zusätzliche MCP-Server im Chat (z. B. von Drittanbietern) werden mit den Erweiterungen in
#79 geklärt ([erweiterungen.md](erweiterungen.md), Punkt 4); Stufe 1 nutzt nur die Werkzeuge der
Suite.

## Entscheidungen

Getroffen am 2026-10-13.

1. **Gedächtnis gleich in Stufe 1** (C.10), nur auf ausdrücklichen Wunsch („Remember …“), als lesbare
   `memory.md`.
2. **Abgeben an Bots nur auf Ansage:** Der Hauptbot fragt einen Bot nur, wenn die Person es sagt
   („frag den Reiseplaner …“). Er darf einen passenden Bot vorschlagen, aber nicht von selbst fragen.
3. **Erster Start:** vorausgewählt ist nur *Read only* für Kalender, Aufgaben und Eingang.
4. **OpenRouter:** nur Modelle mit offenen Gewichten, Filter abschaltbar.
5. **Ein Client „Assistant“** für alle Antriebe, der Antrieb steht im Protokoll.
6. **Dateien und Shell** beim Agent-Antrieb immer aus.
7. **Bereich `assistant` für andere Agenten** standardmäßig aus, auch in *Full access*.
8. **Aufbewahrung** 30 Tage, einstellbar; angeheftete Unterhaltungen bleiben.
9. **Fremde MCP-Server im Chat** nicht in Stufe 1; Klärung in #79.
10. **Mikrofon** einschalten ist neuer Punkt 6 in „Was nie nach außen geht“.
11. **Weckwort** zum Start fest („Hey Torro“), frei wählbar in einer späteren Aufgabe.

Getroffen am 2026-10-14 (Abschnitt „Karten im Chat“).

12. **Karten entstehen auf zwei Wegen:** automatisch nach schreibenden Werkzeugen, bei Suchen über
    `assistant.show_items` (nur Kennungen aus dieser Unterhaltung, max. 10). Aufgelöst werden sie mit
    den Rechten des Assistenten.
13. **Die Entwurfs-Karte ist die Freigabe:** Send = Allow, Discard = Deny. Ohne offene Freigabe sendet
    der Klick direkt. Gilt nur für den angezeigten Stand des Entwurfs; vorerst kein „Undo send“.
14. **Löschen auf der Karte:** Offene Lösch-Freigaben erscheinen als „Delete“/„Keep“ auf der Karte des
    Termins oder der Aufgabe.
