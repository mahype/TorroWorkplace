# Inhaltskarten: eine Gestalt für Assistent, Bots und Chat

Stand: 2026-11-15. **Konzept (Planung #775), ohne Code.** Entwurf:
[`docs/prototypes/inhaltskarten/index.html`](prototypes/inhaltskarten/index.html). Jede Karte, die hier noch
fehlt, wird ein eigenes Issue aus der Liste (Abschnitt „Reihenfolge“).

## Ziel

Bestimmte Inhalte bekommen im Chat eine eigene Darstellung statt nur Text oder Link, und zwar **überall
gleich**: im Assistenten, bei seinen Bots und im Chat mit anderen Leuten. Eine Webseite sieht in jedem der
drei Fenster gleich aus; was darstellbar ist und was noch fehlt, steht an einer Stelle: in der Liste unten.

## Begriffe

- **Karte (card)** bleibt der Name. Code und Doku sagen schon „Karte“ (`CardRef`, `Card`, `chat.show_card`).
  „Artefakt“ aus dem Gespräch ist verworfen: Es sagt nichts über die Darstellung und klingt nach Datei.
- **Landkarte** heißt immer „Landkarte“, nie „Karte“: In den Kontakten ([kontakte.md](kontakte.md), #357)
  und im Termin-Editor ([termin-editor.md](termin-editor.md)) ist „Karte“ schon die Landkarte, und eine
  Landkarten-Karte wäre nicht zu lesen.
- **Inhaltsart:** was eine Karte darstellt (Webseite, Mail, Termin …). Eine Zeile der Liste.
- **Kurzform:** die Karte in einer Zeile, für Mini-Kachel, Schnellfenster und lange Trefferlisten.
- **Quelle der Zusatzinformation:** wer Titel, Bild und Text liefert: der Kern aus seinem Cache, Nextcloud
  (Referenz-API), die Seite selbst oder ein angebundener MCP-Server.

## Bestand

Heute gibt es zwei getrennte Ansätze, die dieses Konzept zusammenführt:

| Ansatz | Wo beschrieben | Gebaut | Entworfen |
|---|---|---|---|
| **Karten des Assistenten** (`EntryContent::Card` mit `CardRef`, aufgelöst von `assistant.get_cards`) | [assistent.md](assistent.md), „Karten im Chat“ | Datei (`CardRef::File`), Mail-Vorschlag (`CardRef::Suggestion`) | Termin, Aufgabe, Mail-Entwurf, Issue-Entwurf; `assistant.show_items` steht noch nicht im Befehlskatalog |
| **Link-Karten und Videokarte im Chat mit Leuten** (Feld `cards` in `chat.get_messages`, Bild über `chat.get_preview`) | [chat.md](chat.md), „Fenster“, Entscheidungen 23 und 25 | Link-Karte (#636), Videokarte und Player für YouTube (#673) | |
| **Teilen** (`ShareCard`: Titel, Kurztext, Datei, Felder) | [teilen.md](teilen.md) | Kontakte | weitere Arten später |

Die bestehenden Karten und der YouTube-Player **bleiben, wie sie sind**. Dieses Konzept sagt nur, auf welche
gemeinsamen Bausteine sie bei der Zusammenführung kommen (Abschnitt „Gemeinsame Gestalt“). `ShareCard` ist
eine andere Sache (Inhalt für ein Ziel wie Mail oder Zwischenablage) und bleibt getrennt.

## Die Liste

Stand je Zeile: `gebaut` (im Code und im Befehlskatalog), `geplant` (entschieden, noch kein Issue gebaut),
`Idee` (offen, ob und wie). „Gilt in“ nennt die Oberflächen, in denen die Karte vorkommen soll: **A**ssistent,
**B**ots, **C**hat mit Leuten. Eine Karte, die im Chat mit Leuten erscheint, erscheint auch im Assistenten
(gleicher Baustein); umgekehrt nicht immer, weil der Chat mit Leuten nicht in fremde Konten greift.

| Inhaltsart | Erkannt an | Zeigt | Zusatzinformation von | Gilt in | Stand |
|---|---|---|---|---|---|
| **Webseite** | `http(s)`-Link | Vorschaubild, Titel, Domain, etwas Text | im Chat: Nextcloud (`references/resolve`) oder, bei Privacy „Always“/„Show once“, die Seite selbst; im Assistenten: die Seite, nur mit Privacy | A B C | Chat `gebaut` (#636), Assistent/Bots `geplant` |
| **YouTube** | `youtube.com/watch`, `youtu.be`, Shorts | Bild in 16:9, Abspielzeichen, Titel, „Open on YouTube“; Klick öffnet den Player | Link-Karte (`video`: Kennung, Start, Short) | A B C | Chat `gebaut` (#673), Assistent/Bots `geplant` |
| **Andere Videos** (Vimeo, PeerTube, direkte Videodatei) | Link auf bekannten Anbieter | wie YouTube, ohne eigenen Player zuerst | Link-Karte | A B C | `Idee` |
| **E-Mail**, gefunden oder referenziert | Treffer aus `mail.search` / `mail.get_message`; im Chat geteilte Mail-Karte | Absender, Betreff, Datum, Anfang des Textes, „Open in mail“; Kennzeichnung `untrusted` | der Kern aus dem Mail-Cache, nie aus dem Text des Modells | A B (C nur als geteilte Karte) | `geplant` |
| **Mail-Vorschlag** (Termin, Aufgabe aus einer Mail) | `mail.extract_action` | der Vorschlag mit „Accept“/„Dismiss“ | `mail.list_suggestions` | A B | `gebaut` (`CardRef::Suggestion`, #605) |
| **Mail-Entwurf** | `mail.save_draft` | Kopfzeilen und Anfang, „Review“, „Send“ nach Freigabe | `mail.get_message` | A B | `geplant` (entworfen in [assistent.md](assistent.md)) |
| **Termin** | `calendar.save_event` / `list_events` | Kalenderblatt, Ort, Teilnehmer, Überschneidung, „Open in calendar“ | `calendar.list_events` | A B C (geteilt) | `geplant` (entworfen) |
| **Aufgabe** | `tasks.save_todo` / `list_todos` | Fälligkeit, Priorität, Liste, „Open in tasks“, „Done“ | `tasks.list_todos` | A B C (geteilt) | `geplant` (entworfen) |
| **Issue-Entwurf** (GitHub) | `tasks.create_issue` (Vorschlag, [github.md](github.md)) | Titel, Repo, „Create“ nach Freigabe | der Kern | A B | `geplant` (entworfen) |
| **Datei** | `tools.create_pdf` u. a. | Name, Größe, Art, „Open“, „Save as…“ | Register der eigenen Werkzeuge | A B | `gebaut` (`CardRef::File`) |
| **Landkarte** | Link auf Google Maps, OpenStreetMap, Apple Maps | Kartenbild, Ort, „Open in maps“ | Ort aus dem Link gelesen (nichts geladen), Bild über `calendar.map_preview` | A B C | `geplant`, siehe „Landkarten“ |
| **PDF** | Link oder Anhang `application/pdf` | Titel, Seitenzahl, Dateigröße; erste Seite als Bild | der Kern liest Eigenschaften der Datei; erste Seite braucht einen Renderer | A B C | Titel und Seitenzahl `geplant`, erste Seite `Idee` |
| **Bild aus einem Programm** (z. B. ArtCraft) | Link oder Verweis eines angebundenen MCP-Servers | kleine Vorschau, Titel, „From <Name> (external, untrusted)“ | der MCP-Server, nur Titel, Kurztext, ein Bild | A B | `geplant`, siehe „MCP als Quelle“ |

Die Liste wächst: Wer eine Inhaltsart vorschlägt, ergänzt eine Zeile (mit Stand `Idee`) und ein Issue folgt aus
der Reihenfolge unten.

## Grundsätze

Sie gelten für jede Karte, in jedem der drei Fenster. Sie stehen schon in
[assistent.md](assistent.md) („Karten im Chat“, „Grundsätze“) und werden hier nur auf alle Oberflächen
ausgedehnt.

- **Die Karte verweist, sie kopiert nicht.** Im Verlauf steht, *welcher* Inhalt gemeint ist; was die Karte zeigt,
  holt der Kern beim Anzeigen frisch. Was auf der Karte steht, kommt vom Kern oder vom genannten Anbieter, nie aus
  dem Text des Modells.
- **Mit den Rechten des Absenders aufgelöst.** Der Assistent sieht mit seinen Rechten, nicht mit denen der App.
  Im Chat mit Leuten gilt: Der Kern löst nur auf, was der Empfänger ohnehin lesen darf. Gesperrt oder gelöscht heißt
  „No longer available“.
- **Knöpfe sind Klicks des Menschen.** Jeder Knopf schickt einen normalen `torro_ipc::Request`, nie ein Werkzeug
  des Modells.
- **Fremder Text bleibt gekennzeichnet.** Mail und Texte von MCP-Servern tragen `untrusted` sichtbar und zählen
  nie als Anweisung.
- **Jede Karte hat eine Kurzform** von einer Zeile.

## Gemeinsame Gestalt

Dieselben Bausteine für alle drei Oberflächen, damit eine Webseite überall gleich aussieht. Jede Oberfläche holt
Inhalt weiter auf ihrem Weg (Assistent: `assistant.get_cards`, Chat: `chat.get_messages` mit `cards`); nur die
Bausteine sind gemeinsam.

| Baustein | Inhalt | Regel |
|---|---|---|
| **Kopf** | Symbol der Art, Quelle (Domain, Konto, „From <Name>“), Kennzeichnung `untrusted` | immer eine Zeile, Quelle nie weglassen |
| **Bild** | Vorschaubild, Kalenderblatt, Kartenbild, Abspielzeichen | höchstens eines; 16:9 für Video, sonst 3:2; fehlt es, steht das Symbol der Art |
| **Titel** | eine Zeile, abgeschnitten | im Chat ohne Titel die Domain |
| **Zusatzzeilen** | höchstens drei Zeilen mit Symbol (Ort, Zeit, Absender, Seiten …) | die wichtigsten zuerst, der Rest in „Open“ |
| **Knöpfe** | ein Haupt-Knopf („Open …“) und höchstens drei weitere | im Chat mit Leuten nur Knöpfe, die nichts schreiben („Open“, „Play“, „Open in <App>“, „Save as…“) |
| **Kurzform** | Symbol, Titel, eine Zusatzangabe in 40 Pixel Höhe | für Mini-Kachel, Schnellfenster und Listen |

Abweichungen sind nur dort erlaubt, wo eine Art es braucht (Kalenderblatt statt Bild beim Termin, Player-Fenster
beim Video). Der Entwurf zeigt jede Inhaltsart im Assistenten und im Chat nebeneinander. Die Bausteine kommen als
gemeinsame Widgets in `torro-gui` (`widgets.rs`); die heutigen Link-Karten und Datei-Karten werden bei der Umstellung auf
sie gelegt, ohne ihre Daten zu ändern.

## Grenzen: Karten oder Liste

Wie viele Treffer als Karte erscheinen, hängt an der Zahl, nicht am Gefühl:

| Treffer | Darstellung |
|---|---|
| 1 bis 3 | volle Karten |
| 4 bis 10 | Liste von Kurzformen, jede Zeile mit „Open“ |
| ab 11 | ein Satz mit der Anzahl und „Show all“, das die passende App öffnet („12 mails match“) |

Zwei Mails sind also Karten, 300 sind ein Satz. Zehn volle Karten sind verworfen: Sie füllen das Fenster. Im Chat
mit Leuten bleibt es beim **ersten Link** einer Nachricht (wie in Talk-Web); mehr als eine Karte unter einer
Nachricht gibt es dort nicht.

## Chat mit Leuten

Der Chat erkennt Mails und Termine **nur aus geteilten Karten und aus Links auf den eigenen Nextcloud-Server**.
Er rät nicht aus freiem Text: Eine fremde Mail liegt nicht im eigenen Postfach, und ein Datum im Satz ist kein
Termin. Webseiten, Videos und Landkarten erkennt er am Link wie heute die Link-Karte (Entscheidung 23).
Wer Mail oder Termin teilen will, teilt eine Karte über „Share…“ ([teilen.md](teilen.md)); der Kern baut sie aus
seinem Cache.

## Landkarten

Die erste Fassung kennt **Google Maps, OpenStreetMap und Apple Maps**. Den Ort liest die Karte aus dem Link
(Koordinaten, Adresse, Suchbegriff), ohne etwas zu laden. Das Bild kommt über `calendar.map_preview`
([termin-editor.md](termin-editor.md), Punkt 9) und ist **von zwei Dingen abhängig**:

1. Der Schalter `maps` ist an (aus, bis ein Mensch ihn einschaltet) und der Dienst für Kacheln erlaubt.
2. Der Anbieter des Links ist in Settings › Privacy erlaubt.

Fehlt eines, zeigt die Karte Ort und „Open in maps“ ohne Bild. Weitere Anbieter (Bing, HERE, Komoot …) kommen
später. Offen: ob der Schalter `maps` in Settings › Privacy aufgeht (siehe „Offene Fragen“).

## MCP als Quelle

Ein angebundener MCP-Server kann Zusatzinformation zu einem geposteten Inhalt liefern, etwa ArtCraft eine Vorschau
seines Bildes. Das ist die gefährlichste Quelle, weil ihr Text von außen kommt. Darum gilt eng:

- **Nur aus dem Ergebnis eines erlaubten Werkzeugs dieser Unterhaltung.** Der Server wird nicht von sich aus
  gefragt und nicht für jeden Link; die Karte entsteht, wenn der Assistent oder ein Bot ein Werkzeug des Servers
  aufgerufen hat, das der Mensch freigegeben hat.
- **Nur Titel, Kurztext und ein Bild.** Titel höchstens 200, Kurztext höchstens 500 Zeichen; ein Bild als PNG,
  JPEG oder WebP bis 1 MiB, **kein SVG** (kann Skript und Verweise tragen). Mehr wird abgeschnitten.
- **Sichtbar fremd:** der Kopf trägt „From <Name> (external, untrusted)“. Der Text ist Daten, nie Anweisung: Er
  geht nicht ins Modell zurück als Auftrag.
- **Stufe Lesen reicht.** Der Server braucht keine Schreibrechte, um eine Karte zu liefern; die Rechte des
  Servers sind die seines Werkzeugs (siehe [erweiterungen.md](erweiterungen.md)), eine Karte vergibt keine.
- **Keine eigene Oberfläche.** `ui://`-Ressourcen eines Servers werden abgelehnt: Fremder Code im Chat könnte
  Anweisungen einschleusen und mitlesen.
- **Nur Assistent und Bots**, nicht der Chat mit Leuten: Dort gibt es kein Werkzeug des Menschen, das den Server
  gerufen hätte.

## Datenschutz

Karten fremder Anbieter folgen **Settings › Privacy** wie die Link-Karten heute ([settings.md](settings.md),
„Privacy“; [chat.md](chat.md), Entscheidung 23): immer, nach Rückfrage (Standard) oder nie, je Anbieter.
Bei „Ask“ steht statt der Karte der ruhige Satz „This message shows content from <Anbieter>.“ mit „Show once“,
„Always allow <Anbieter>“ und „Never for <Anbieter>“.

- **Neu für den Assistenten:** Er lädt bisher nichts von fremden Servern. Mit der Webseiten-Karte prüft er zum
  ersten Mal Settings › Privacy; die Rückfrage erscheint als Karte im Verlauf.
- **Mail-Karten** sind `untrusted` und laden **nie Bilder aus dem Netz**; für Mail gilt die eigene Regel der
  externen Bilder.
- **Landkarten** hängen zusätzlich am Schalter `maps` (Abschnitt „Landkarten“).
- **Agenten verwalten das nie:** Stufe und Anbieterliste ändert nur ein Mensch ([api.md](api.md), Punkt 14).

## Drei Wege

Jede Funktion gibt es in Oberfläche, API und MCP ([api.md](api.md)). Was fehlt, ist als **Vorschlag** markiert
und kommt mit dem Issue der jeweiligen Karte in den Befehlskatalog (`torro-ipc`, `api.rs`), im ersten Schritt.

| Funktion | Oberfläche | API | Zugriff | MCP |
|---|---|---|---|---|
| Karten eines Assistenten-Verlaufs lesen | Chatfenster des Assistenten | `assistant.get_cards` (vorhanden) | Read | `assistant_get_cards` (vorhanden) |
| Link-Karten einer Nachricht lesen | Chatfenster | Feld `cards` in `chat.get_messages` (vorhanden) | Read | `chat_get_messages` (vorhanden) |
| Bild einer Karte holen | Chatfenster | `chat.get_preview { card: true }` (vorhanden) | Read | `chat_get_preview` (vorhanden) |
| Link-Karte einmal zeigen („Show once“) | Chatfenster | `chat.show_card` (vorhanden) | Write | `Never`: nur Menschen, [api.md](api.md) Punkt 14 |
| Stufe und Anbieter ändern | Settings › Privacy | `settings.set_setting privacy_*` (vorhanden) | Admin, nur App-Schlüssel | im Katalog `Mcp::Tool`; Werte `privacy_*` bekommen von jedem Client `forbidden` (`kept_for_people`, [api.md](api.md) Punkt 14), Lesen über `settings_get_settings` |
| Webseiten-Karte im Assistenten auflösen | Chatfenster des Assistenten | Vorschlag: `assistant.get_cards` mit `CardRef::Link` | Read | vorhandenes `assistant_get_cards` |
| Gefundene Mail als Karte | Chatfenster des Assistenten | Vorschlag: `assistant.get_cards` mit `CardRef::Mail` | Read | vorhandenes `assistant_get_cards` |
| Landkarte als Karte | beide Chatfenster | Vorschlag: `CardRef::Map`, Bild über `calendar.map_preview` (vorhanden); der Kern holt es beim Auflösen der Karte selbst, das Modell sieht es nie | Read | Bild nicht für Assistenten (`calendar_map_preview`, [termin-editor.md](termin-editor.md)) |
| Karte eines MCP-Servers | Chatfenster des Assistenten | Vorschlag: `CardRef::External` aus dem Werkzeugergebnis | Read | über das Werkzeug des Servers; kein eigenes Tool |
| Karte zusammenführen (Kurzform) | alle Fenster | Teil derselben Antworten | Read | wie oben |

Es entsteht **kein Befehl, mit dem ein Agent eine Karte frei hinstellt** (`chat.show_card` ist bewusst `Never`):
Eine Karte entsteht nur als Ergebnis eines Befehls oder Werkzeugs, das der Kern selbst ausführt.

## Reihenfolge

Vorschlag für die Folge-Issues, je ein Titel, hier nicht angelegt:

1. **Inhaltskarten: gemeinsame Bausteine und Webseiten-Karte** (Widgets nach „Gemeinsame Gestalt“, Link-Karte
   und Datei-Karte darauf legen, Webseite im Assistenten mit Privacy-Rückfrage).
2. **Inhaltskarten: gefundene Mail als Karte** (`CardRef::Mail`, Kurzform, Grenzen aus „Grenzen“).
3. **Inhaltskarten: Termin, Aufgabe und Mail-Entwurf** (die in [assistent.md](assistent.md) entworfenen Karten
   auf den Bausteinen bauen).
4. **Inhaltskarten: Videos** (YouTube im Assistenten, andere Anbieter).
5. **Inhaltskarten: Landkarten** (Google Maps, OpenStreetMap, Apple Maps).
6. **Inhaltskarten: PDF** (Titel und Seitenzahl, erste Seite nach Entscheidung zum Renderer).
7. **Inhaltskarten: Bild eines MCP-Servers** (ArtCraft als erster Fall, nach „MCP als Quelle“).

Grund der Reihenfolge: Die Bausteine (1) braucht jede andere Karte; die Mail (2) ist der häufigste Fall des
Assistenten; Termin und Aufgabe (3) sind schon entworfen; MCP (7) kommt zuletzt, weil es die meisten
Sicherheitsfragen hat.

## Entscheidungen (Planung #775)

1. **Bei „Karte“ bleiben**, „Artefakt“ verworfen; die Landkarte heißt immer „Landkarte“.
2. **Grenzen:** bis 3 Treffer volle Karten, 4 bis 10 Liste von Kurzformen, ab 11 ein Satz mit „Show all“. Zehn
   volle Karten verworfen.
3. **Chat mit Leuten:** Mail und Termin nur aus geteilten Karten und Links auf den eigenen Nextcloud-Server, nicht
   aus freiem Text. Verworfen: Raten aus dem Text (eine fremde Mail liegt nicht im eigenen Postfach).
4. **Landkarten, erste Fassung:** Google Maps, OpenStreetMap, Apple Maps; Ort aus dem Link, Bild nur mit Schalter
   `maps` und erlaubtem Anbieter. Weitere Anbieter später.
5. **MCP als Quelle:** nur Titel, Kurztext, ein Bild, sichtbar fremd, nur Assistent und Bots, `ui://`
   abgelehnt. Verworfen: eigene Oberflächen eines Servers im Chat.

## Offene Fragen

- **`maps` in Privacy?** Der Schalter für Landkartenbilder steht heute neben Settings › Privacy, nicht darin
  ([termin-editor.md](termin-editor.md), Punkt 9). Geht er in Privacy auf, gäbe es einen Ort für alles, was
  fremde Server fragt; dagegen spricht, dass `maps` auch getippte Adressen an einen Suchdienst schickt.
- **Renderer für die erste PDF-Seite:** Es gibt keinen in der Suite; ein zusätzlicher Baustein braucht eine
  eigene Entscheidung (Größe, Lizenz, Sicherheit bei fremden Dateien).
- **Weitere Videoanbieter:** welche, und ob ein Anbieter ohne Player-Fenster nur öffnet.
