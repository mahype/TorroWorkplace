# Inhaltskarten: eine Gestalt für Assistent, Bots und Chat

Stand: 2026-11-15. **Konzept (Planung #775); erste Welle gebaut (#784: #785, #786, #797, #789, #790).** Entwurf:
[`docs/prototypes/inhaltskarten/index.html`](prototypes/inhaltskarten/index.html), Mockups im Artifact
[„Vorschaukarten im Chat“](https://claude.ai/artifact/5BBebPFaWuBQ3oSPQq22yo). Jede Karte, die hier noch fehlt,
wird ein eigenes Issue aus der Liste (Abschnitt „Reihenfolge“).

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
| **Webseite** | `http(s)`-Link | Vorschaubild, Titel, Domain, etwas Text | im Chat: Nextcloud (`references/resolve`) oder, bei Privacy „Always“/„Show once“, die Seite selbst; im Assistenten: die Seite, nur mit Privacy | A B C | `gebaut` (Chat #636, Assistent/Bots #785) |
| **YouTube** | `youtube.com/watch`, `youtu.be`, Shorts | Bild in 16:9, Abspielzeichen, Titel, „Open on YouTube“; Klick öffnet den Player | Link-Karte (`video`: Kennung, Start, Short) | A B C | Chat `gebaut` (#673), Assistent/Bots `geplant` |
| **Andere Videos** (Vimeo, PeerTube, direkte Videodatei) | Link auf bekannten Anbieter | wie YouTube, ohne eigenen Player zuerst | Link-Karte | A B C | `Idee` |
| **E-Mail**, gefunden oder referenziert | Treffer aus `mail.search` / `mail.get_message`; im Chat geteilte Mail-Karte | Absender, Betreff, Datum, Anfang des Textes, „Open in mail“; Kennzeichnung `untrusted` | der Kern aus dem Mail-Cache, nie aus dem Text des Modells | A B (C nur als geteilte Karte) | A B `gebaut` (#786), C geteilt `geplant` (#800) |
| **Mail-Vorschlag** (Termin, Aufgabe aus einer Mail) | `mail.extract_action` | der Vorschlag mit „Accept“/„Dismiss“ | `mail.list_suggestions` | A B | `gebaut` (`CardRef::Suggestion`, #605) |
| **Mail-Entwurf** | `mail.save_draft` | Kopfzeilen und Anfang, „Review“, „Send“ nach Freigabe | `mail.get_message` | A B | `gebaut` (#797) |
| **Termin** | `calendar.save_event` / `list_events` | Kalenderblatt, Ort, Teilnehmer, Überschneidung, „Open in calendar“ | `calendar.list_events` | A B C (geteilt) | A B `gebaut` (#797), C geteilt `geplant` (#800) |
| **Aufgabe** | `tasks.save_todo` / `list_todos` | Fälligkeit, Priorität, Liste, „Open in tasks“, „Done“ | `tasks.list_todos` | A B C (geteilt) | A B `gebaut` (#797), C geteilt `geplant` (#800) |
| **Issue-Entwurf** (GitHub) | `tasks.create_issue` (Vorschlag, [github.md](github.md)) | Titel, Repo, „Create“ nach Freigabe | der Kern | A B | `geplant` (entworfen) |
| **Datei** | `tools.create_pdf` u. a. | Name, Größe, Art, „Open“, „Save as…“ | Register der eigenen Werkzeuge | A B | `gebaut` (`CardRef::File`) |
| **Landkarte** | Link auf Google Maps, OpenStreetMap, Apple Maps | Kartenbild, Ort, „Open in maps“ | Ort aus dem Link gelesen (nichts geladen), Bild über `calendar.map_preview` | A B C | `geplant`, siehe „Landkarten“ |
| **PDF** | Link oder Anhang `application/pdf` | Titel, Seitenzahl, Dateigröße; erste Seite als Bild | der Kern liest Eigenschaften der Datei; erste Seite über den Vorschau-Anbieter für PDF (Renderer offen) | A B C | Titel und Seitenzahl `geplant`, erste Seite `Idee`, siehe „Vorschau-Anbieter je Dateiart“ |
| **Bild aus einem Programm** (z. B. ArtCraft) | Link oder Verweis eines angebundenen MCP-Servers | kleine Vorschau, Titel, „From <Name> (external, untrusted)“ | der MCP-Server, nur Titel, Kurztext, ein Bild | A B | `geplant`, siehe „MCP als Quelle“ |
| **Foto und Album** | Treffer aus `photos.list_photos` / `photos.list_albums`; im Chat geteilte Karte | Bild (Album: Titelbild und Anzahl), Datum, Ort, „Open in photos“ | Bild über `photos.get_preview` ([fotos.md](fotos.md), PH1), nur mit dem Recht „Bildinhalte“; ohne das Recht Datum und Ort mit dem Symbol der Art | A B C (geteilt) | `geplant` (#799, wartet auf Fotos im Kern) |
| **Notiz** | Treffer aus `notes.list_notes` / `notes.get_note`; im Chat geteilte Karte | Titel, Kategorie, Anfang des Textes, „Open in notes“ | `notes.get_note` ([notizen.md](notizen.md)) | A B C (geteilt) | `geplant` (#799, wartet auf Notizen im Kern) |
| **Kontakt** | Treffer aus `contacts.list_contacts` / `contacts.get_contact`; im Chat geteilte Kontakt-Karte | Foto, Name, Firma, eine Angabe (Mail oder Telefon), „Open in contacts“ | `contacts.get_contact`, Foto über `contacts.get_photo`; baut auf der geteilten Karte aus [teilen.md](teilen.md) auf | A B C (geteilt) | A B `gebaut` (#789), C geteilt `geplant` (#800) |
| **Nextcloud-Datei** | Link auf den eigenen Nextcloud-Server (`/f/<id>`, `/s/<token>`, `/index.php/apps/files`) | Vorschaubild, Name, Größe, Ordner, „Open in Nextcloud“ | Vorschau vom eigenen Server; keine Privacy-Frage, weil kein fremder Anbieter gefragt wird | A B C | `gebaut` (#790) |
| **GitHub Issue / PR** | Link `github.com/<owner>/<repo>/issues/<nr>` oder `/pull/<nr>` | Nummer, Titel, Repo, Zustand (offen, geschlossen, gemergt), „Open on GitHub“ | der Kern über das verbundene GitHub-Konto ([github.md](github.md)); ohne Konto die Webseiten-Karte | A B C | `gebaut` (#790) |
| **Zitat einer Chat-Nachricht** | Link auf eine Nachricht des eigenen Servers; im Assistenten Treffer aus `chat.search_messages` | Absender, Unterhaltung, Zeit, Anfang der Nachricht, „Open in chat“ | `chat.get_messages`, nur wenn der Empfänger die Unterhaltung lesen darf | A B C | `gebaut` (#790) |

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
| **Laden / Nicht erreichbar** | Platzhalter in der Größe der fertigen Karte; danach „Not available right now“ mit „Try again“ | die Karte springt nicht: Der Platz steht, bevor der Inhalt kommt; Kopf und Quelle stehen auch im Fehlerfall; „No longer available“ (gelöscht, gesperrt) bleibt davon getrennt und hat keinen Knopf |

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
mit Leuten zeigt eine Nachricht **bis zu drei Karten**, in der Reihenfolge der Links; ab dem vierten Link stehen
die übrigen nur als Links im Text. Bisher galt nur der erste Link (wie in Talk-Web); das reichte nicht, sobald eine
Nachricht ein Issue und einen PR oder zwei Dateien nennt. Die Grenze gilt je Nachricht, nicht je Unterhaltung.

## Chat mit Leuten

Der Chat erkennt Mails und Termine **nur aus geteilten Karten und aus Links auf den eigenen Nextcloud-Server**.
Er rät nicht aus freiem Text: Eine fremde Mail liegt nicht im eigenen Postfach, und ein Datum im Satz ist kein
Termin. Webseiten, Videos und Landkarten erkennt er am Link wie heute die Link-Karte (Entscheidung 23), ebenso
Dateien und Nachrichten des eigenen Nextcloud-Servers und GitHub Issues / PRs.
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

## Vorschau-Anbieter je Dateiart

**Grundsatz: Vorschaukarten gelten zuerst für eigene Inhalte aus Torro** (Mails, Termine, Fotos, Notizen,
Dateien der eigenen Konten und des eigenen Nextcloud-Servers). Fremde Seiten und fremde Programme kommen danach
und nur mit den Regeln aus „Datenschutz“ und „MCP als Quelle“.

Welches Bild eine Datei bekommt, entscheidet eine **Zuordnung Dateiart → Anbieter** im Kern. Die Dateiart ist der
Medientyp (`application/pdf`, `image/png` …), bei unbekanntem Typ die Endung. Je Art gilt der erste Anbieter, der
eine Vorschau liefert:

1. **Eigene Renderer der Suite.** Der Kern selbst (verkleinerte Bilder PNG, JPEG, WebP), die Fotos-App
   (`photos.get_preview`), der eigene Nextcloud-Server für seine Dateien. Ein eigener Renderer trägt sich mit
   seiner App für die Arten ein, die er kann.
2. **Freigegebene MCP-Server.** Ein Server, den ein Mensch angebunden und dessen Werkzeuge er freigegeben hat
   ([erweiterungen.md](erweiterungen.md)), kann sich für eine Art eintragen, etwa ArtCraft für seine Projektdateien.
   Er gilt **nur für Dateien aus dem Ergebnis eines seiner freigegebenen Werkzeuge in dieser Unterhaltung**, nur in
   Assistent und Bots: Der Kern fragt ihn nie von sich aus und nie für eine Datei, die von anderswo kommt (Regel aus
   „MCP als Quelle“). Der Eintrag sagt also nur, *welche* Art der Server bebildern darf, nicht, dass er gefragt wird.
   Er liefert nur ein Bild nach den Regeln aus „MCP als Quelle“: PNG, JPEG oder WebP bis 1 MiB, **kein SVG**, und
   der Kopf der Karte trägt „From <Name> (external, untrusted)“. Ein eigener Renderer geht vor, ein Server
   überschreibt ihn nicht.
3. **Rückfall: das Symbol der Art.** Gibt es keinen Anbieter oder antwortet keiner, zeigt die Karte das Symbol
   (Baustein „Bild“) und alles Übrige wie sonst. Eine Karte wartet nie auf einen Anbieter (Baustein „Laden / Nicht
   erreichbar“).

Die **erste PDF-Seite** ist damit ein Fall dieses Weges: Sie braucht einen Anbieter für `application/pdf`. Welcher
Renderer das wird, bleibt offen (siehe „Offene Fragen“), Stand weiter `Idee`. Ein MCP-Server ersetzt diesen Renderer
nicht: Er bebildert nur PDFs, die sein eigenes Werkzeug geliefert hat, und nur in Assistent und Bots.

- **Eintragen und Reihenfolge ändert nur ein Mensch** (Settings › Extensions). Ein Agent sieht die Zuordnung, ändert
  sie aber nie: Sonst könnte er sich selbst zum Anbieter für fremde Dateien machen.
- **Im Chat mit Leuten nur eigene Renderer:** Dort gibt es kein Werkzeug, das den Server gerufen hätte, und er
  sähe sonst Dateien anderer Leute.
- Ein Anbieter bekommt **nur die eine Datei**, die angezeigt wird, nie einen Ordner oder eine Liste.

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
| Bild einer Karte holen | Chatfenster | `chat.get_preview { card: true, index? }` (bis drei Karten je Nachricht, #785) | Read | `chat_get_preview` (vorhanden) |
| Bild einer Webseiten-Karte im Assistenten | Chatfenster des Assistenten | `assistant.get_card_picture { conversation, seq }` (#785) | Read | `Never`: nur das Fenster braucht die Bilddaten; Agenten lesen Titel und Text über `assistant_get_cards` |
| Link-Karte einmal zeigen („Show once“) | Chatfenster | `chat.show_card { index? }` (vorhanden) | Write | `Never`: nur Menschen, [api.md](api.md) Punkt 14 |
| Webseiten-Karte im Assistenten einmal zeigen („Show once“) | Chatfenster des Assistenten | `assistant.show_card { conversation, seq }` (#785) | Write | `Never`: nur Menschen, [api.md](api.md) Punkt 14 |
| Stufe und Anbieter ändern | Settings › Privacy | `settings.set_setting privacy_*` (vorhanden) | Admin, nur App-Schlüssel | im Katalog `Mcp::Tool`; Werte `privacy_*` bekommen von jedem Client `forbidden` (`kept_for_people`, [api.md](api.md) Punkt 14), Lesen über `settings_get_settings` |
| Webseiten-Karte im Assistenten auflösen | Chatfenster des Assistenten | `assistant.get_cards` mit `CardRef::Link` (#785) | Read | vorhandenes `assistant_get_cards` |
| Gefundene Mail als Karte | Chatfenster des Assistenten | `assistant.get_cards` mit `CardRef::Mail` (#786) | Read | vorhandenes `assistant_get_cards` |
| Treffer als Karten zeigen (bis 10, Kurzform ab 4, „Show all“ ab 11) | Chatfenster des Assistenten, vom Assistenten selbst | `assistant.show_items { conversation?, items }` (#786); nur Kennungen, die der Assistent in dieser Unterhaltung gelesen hat | Write | `assistant_show_items` |
| Landkarte als Karte | beide Chatfenster | Vorschlag: `CardRef::Map`, Bild über `calendar.map_preview` (vorhanden); der Kern holt es beim Auflösen der Karte selbst, das Modell sieht es nie | Read | Bild nicht für Assistenten (`calendar_map_preview`, [termin-editor.md](termin-editor.md)) |
| Karte eines MCP-Servers | Chatfenster des Assistenten | Vorschlag: `CardRef::External` aus dem Werkzeugergebnis | Read | über das Werkzeug des Servers; kein eigenes Tool |
| Karte zusammenführen (Kurzform) | alle Fenster | Teil derselben Antworten | Read | wie oben |
| Vorschau-Anbieter je Dateiart lesen | Settings › Extensions | Vorschlag: `extensions.list_preview_providers` (Art, Anbieter, Reihenfolge) | Read | Vorschlag: `extensions_list_preview_providers` |
| Vorschau-Anbieter eintragen, entfernen, ordnen | Settings › Extensions | Vorschlag: `extensions.set_preview_provider { kind, provider, position? }` | Admin, nur App-Schlüssel | im Katalog `Mcp::Tool`, jeder Client bekommt `forbidden` (`kept_for_people` wie `privacy_*`); Grund: ein Agent machte sich sonst selbst zum Anbieter (siehe „Offene Fragen“) |
| Vorschaubild einer Datei holen | alle Fenster | Teil der vorhandenen `assistant.get_cards` (`CardRef::File`) und `chat.get_preview { card: true }`; Vorschlag: der Kern fragt dabei den Anbieter der Dateiart | Read | vorhandene `assistant_get_cards`, `chat_get_preview`; kein eigenes Tool, das Bild entsteht nur beim Auflösen einer Karte |
| Foto und Album als Karte | beide Chatfenster | Vorschlag: `CardRef::Photo` / `CardRef::Album`, Bild über `photos.get_preview` (Vorschlag in [fotos.md](fotos.md), PH1) | Read **und** Recht „Bildinhalte“ für das Bild | vorhandenes `assistant_get_cards`; Bild über `photos_get_preview` nur mit dem Recht |
| Notiz als Karte | beide Chatfenster | Vorschlag: `CardRef::Note`, Inhalt über `notes.get_note` ([notizen.md](notizen.md)) | Read | vorhandenes `assistant_get_cards` |
| Kontakt als Karte | beide Chatfenster | `CardRef::Contact` (#789) über `contacts.get_contact` und `contacts.get_photo` (vorhanden); im Chat die geteilte Karte aus [teilen.md](teilen.md) | Read | vorhandenes `assistant_get_cards` |
| Nextcloud-Datei als Karte | beide Chatfenster | Link-Karte mit `kind: nextcloud_file` im Feld `cards` von `chat.get_messages`, im Assistenten `CardRef::Link` | Read | vorhandene `chat_get_messages`, `assistant_get_cards` |
| GitHub Issue / PR als Karte | beide Chatfenster | Link-Karte mit `kind: github`, aufgelöst über das GitHub-Konto des Kerns ([github.md](github.md)) | Read | vorhandene `chat_get_messages`, `assistant_get_cards` |
| Zitat einer Chat-Nachricht als Karte | beide Chatfenster | Link-Karte mit `kind: chat_message`, Inhalt über `chat.get_messages` (vorhanden), nur mit Leserecht des Empfängers | Read | vorhandene `chat_get_messages`, `assistant_get_cards` |
| Karte neu laden („Try again“) | alle Fenster | kein eigener Befehl: dieselbe Anfrage (`assistant.get_cards`, `chat.get_preview`) noch einmal | Read | wie oben |

Es entsteht **kein Befehl, mit dem ein Agent eine Karte frei hinstellt** (`chat.show_card` und
`assistant.show_card` sind bewusst `Never`): Eine Karte entsteht nur als Ergebnis eines Befehls oder Werkzeugs, das
der Kern selbst ausführt. `assistant.show_items` wählt nur aus, was der Kern in dieser Unterhaltung schon gelesen hat.

## Reihenfolge

Vorschlag für die Folge-Issues, je ein Titel, hier nicht angelegt:

1. **Inhaltskarten: gemeinsame Bausteine und Webseiten-Karte** (Widgets nach „Gemeinsame Gestalt“ samt „Laden /
   Nicht erreichbar“, Link-Karte und Datei-Karte darauf legen, Webseite im Assistenten mit Privacy-Rückfrage, bis zu
   drei Karten je Chat-Nachricht).
2. **Inhaltskarten: gefundene Mail als Karte** (`CardRef::Mail`, Kurzform, Grenzen aus „Grenzen“).
3. **Inhaltskarten: Termin, Aufgabe und Mail-Entwurf** (die in [assistent.md](assistent.md) entworfenen Karten
   auf den Bausteinen bauen).
4. **Inhaltskarten: Notiz, Kontakt, Foto und Album** (eigene Inhalte aus Torro; Kontakt baut auf der geteilten
   Karte auf, Foto auf `photos.get_preview` und dem Recht „Bildinhalte“). Gebaut ist der Kontakt (#789); Notiz,
   Foto und Album folgen in #799, sobald der Kern Notizen und Fotos hat.
5. **Inhaltskarten: Links auf den eigenen Server und GitHub** (Nextcloud-Datei, Zitat einer Chat-Nachricht,
   GitHub Issue / PR).
6. **Inhaltskarten: Vorschau-Anbieter je Dateiart** (Zuordnung im Kern, eigene Renderer, Settings › Extensions,
   `extensions.list_preview_providers` und `extensions.set_preview_provider`).
7. **Inhaltskarten: Videos** (YouTube im Assistenten, andere Anbieter).
8. **Inhaltskarten: Landkarten** (Google Maps, OpenStreetMap, Apple Maps).
9. **Inhaltskarten: PDF** (Titel und Seitenzahl, erste Seite als Anbieter für `application/pdf` nach Entscheidung
   zum Renderer).
10. **Inhaltskarten: Bild eines MCP-Servers** (ArtCraft als erster Fall, nach „MCP als Quelle“; MCP-Server als
    Vorschau-Anbieter).

Grund der Reihenfolge: Die Bausteine (1) braucht jede andere Karte; die Mail (2) ist der häufigste Fall des
Assistenten; Termin und Aufgabe (3) sind schon entworfen; eigene Inhalte (4, 5) gehen vor fremden, wie es der
Grundsatz aus „Vorschau-Anbieter je Dateiart“ verlangt; die Zuordnung (6) kommt vor PDF (9) und MCP (10), die auf ihr aufbauen; MCP
kommt zuletzt, weil es die meisten Sicherheitsfragen hat.

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
  eigene Entscheidung (Größe, Lizenz, Sicherheit bei fremden Dateien). Er wird der eigene Vorschau-Anbieter für
  `application/pdf`; bis dahin bleibt das Symbol der Art.
- **Weitere Videoanbieter:** welche, und ob ein Anbieter ohne Player-Fenster nur öffnet.
- **Vorschau-Anbieter eintragen: `kept_for_people` oder „Was nie nach außen geht“?** Vorschlag ist der Weg der
  Werte `privacy_*` (im Katalog `Mcp::Tool`, jeder Client bekommt `forbidden`). Gehört das Eintragen eher zu den
  Freigaben ([api.md](api.md), „Was nie nach außen geht“, Punkt 3), wäre es ein neuer Punkt dort und MCP `Never`.
- **Nextcloud-Vorschau für fremde Server:** Links auf einen Nextcloud-Server, der nicht der eigene ist, sind eine
  Webseite und laufen über Privacy; ob föderierte Freigaben (`/s/<token>` eines anderen Servers) wie eigene gelten,
  ist offen.
- **GitHub ohne Konto:** ob die Karte dann nur die Webseiten-Karte ist oder die öffentliche API ohne Anmeldung
  fragt (dann ein fremder Anbieter mit Privacy-Rückfrage).
