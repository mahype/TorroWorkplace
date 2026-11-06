# Chat: Kontaktliste, Chatfenster und Anrufe auf Nextcloud Talk

Stand: 2026-11-06. **Konzept (#470), löst den Text-Chat-Plan aus #201 ab.** Machbarkeit:
[chat-machbarkeit.md](chat-machbarkeit.md). Entwürfe: [prototypes/chat/](prototypes/chat/index.html) (zum Durchsehen
auch als [Artifact](https://claude.ai/artifact/8qB44DohpPixdfsPkzSNNP)). Recherche zu
Status und Sichtbarkeit: [praesenz.md](praesenz.md). Aufgaben gibt es noch keine; die Umsetzung kommt als eigene
Issues je Stufe. Beantwortet die Ideen „Chat (Nextcloud Talk)“ und „Präsenz“ aus dem
[App-Katalog](app-catalog.md): **Wer ist gerade da, wer hat mir geschrieben, und wie erreiche ich ihn sofort?**

## Ziel

Ein leichter Chat der Suite auf **Nextcloud Talk**, schlicht wie früher ICQ, kein zweites Slack:

1. **Eine Kontaktliste** in einem eigenen, schmalen Fenster: alle Menschen der eigenen Nextcloud mit ihrem
   Status (online, abwesend, nicht stören, offline) und ihrer Statusnachricht. Wer online ist, steht oben. Unten
   der eigene Status.
2. **Ein Chatfenster je Unterhaltung.** Ein Doppelklick auf einen Namen öffnet es; ist es schon offen, kommt es
   nach vorn. Darin Verlauf, Antwortfeld und die Knöpfe für Anrufe.
3. **Anrufe zu zweit**, mit Bild oder nur Ton, mit eigener Wahl von Kamera, Mikrofon und Lautsprecher. Wie sie
   technisch laufen, steht unter [Anrufe](#anrufe-stufe-5); die Entscheidung dazu unter
   [Entscheidungen](#entscheidungen), Punkt 13.
4. **Schnell antworten, wo die Nachricht ankommt.** Ungelesene Unterhaltungen stehen schon heute im Notification
   Hub (Eingangsquelle `talk:<login>@<host>`, Aufgabe E5.9). Dort kommen Verlauf und Antwortfeld dazu.

Dazu kommt das **Teilen in einen Chat** ([teilen.md](teilen.md), TL2): Kontakte und später Notizen, Termine und
Fotos als Karte mit Datei in eine Unterhaltung geben.

Wie jede Funktion gibt es den Chat dreimal: in der Oberfläche (Kontaktliste, Chatfenster, Notification Hub), über
die API (Bereiche `chat` und `presence`) und über MCP (`chat_*`, `presence_*`). Ein Agent kann also „Was hat Anna
im Team-Chat zum Plan geschrieben?“ beantworten, „Ist Ben gerade da?“ prüfen und eine Antwort vorschlagen.
Schreiben ist **nach außen** und wartet ab Werk auf eine Freigabe, wie Mail senden. **Anrufe gibt es für Agenten
nicht** (Entscheidung 14).

## Was Talk bietet

Talk (App `spreed`) spricht OCS unter `/ocs/v2.php/apps/spreed/api/…`, angemeldet mit Loginname und
App-Passwort wie alles andere. Funktionen erkennen wir über die Capabilities (`spreed.features`,
`spreed.config`), nicht über Versionsnummern. Was für Status und Anrufe dazukommt und wie es auf dem eigenen
Server aussieht, steht in [chat-machbarkeit.md](chat-machbarkeit.md). Quellen:
[Chat-API](https://nextcloud-talk.readthedocs.io/en/latest/chat/),
[Call-API](https://nextcloud-talk.readthedocs.io/en/latest/call/),
[Capabilities](https://nextcloud-talk.readthedocs.io/en/latest/capabilities/),
[Konstanten](https://nextcloud-talk.readthedocs.io/en/latest/constants/),
[openapi.json](https://github.com/nextcloud/spreed/blob/main/openapi.json).

| Was | Weg | Capability |
|---|---|---|
| Unterhaltungen | `GET /api/v4/room?noStatusUpdate=1` (gibt es: `torro-nextcloud/src/talk.rs`) | – |
| Nachrichten lesen | `GET /api/v1/chat/{token}?lookIntoFuture=0&limit=…&lastKnownMessageId=…` | – |
| Auf Neues warten | dasselbe mit `lookIntoFuture=1&timeout=30` (Long Polling, höchstens 60 s) | – |
| Nichts nebenbei ändern | `setReadMarker=0`, `markNotificationsAsRead=0`, `noStatusUpdate=1` | `chat-keep-notifications` |
| Senden | `POST /api/v1/chat/{token}` mit `message`, `replyTo`, `referenceId`, `silent` | `chat-reference-id`, `silent-send` |
| Gelesen / ungelesen | `POST` bzw. `DELETE /api/v1/chat/{token}/read` (gelesen gibt es schon) | `chat-read-marker`, `chat-unread` |
| Erwähnen | `GET /api/v1/chat/{token}/mentions?search=…`, im Text `@"<mentionId>"` | – |
| Bearbeiten | `PUT /api/v1/chat/{token}/{id}`, nur 24 Stunden lang | `edit-messages` |
| Löschen | `DELETE /api/v1/chat/{token}/{id}`, nur 6 Stunden lang | `delete-messages` |
| Reaktionen | `POST`/`DELETE /api/v1/reaction/{token}/{id}` mit `reaction` | `reactions` |
| Datei teilen | Datei per WebDAV in den Anhangsordner, dann `POST /ocs/v2.php/apps/files_sharing/api/v1/shares` mit `shareType=10`, `shareWith=<token>`, `path`, `talkMetaData={"caption": …}` | `media-caption` |
| Anhangsordner | `spreed.config.attachments.folder` (je Nutzer, meist `/Talk`), `attachments.allowed` | – |
| Höchstlänge | `spreed.config.chat.max-length` (heute 32.000 Zeichen) | – |
| 1:1 anlegen | `POST /api/v4/room` mit `roomType=1`, `invite=<user-id>`; gibt die vorhandene zurück | – |
| Status anderer | App User status: `GET /ocs/v2.php/apps/user_status/api/v1/statuses`, einzeln `…/statuses/{userId}` ([praesenz.md](praesenz.md)) | `user_status.enabled` |
| Eigener Status, Heartbeat | `PUT …/user_status/status`, `…/message/custom`, `PUT …/heartbeat` | `user_status.enabled` |
| Läuft ein Anruf? | Feld `hasCall` und `callFlag` der Unterhaltung (Liste oben) | – |
| Anruf beitreten, verlassen | `POST` bzw. `DELETE /api/v4/call/{token}` (`flags`: Ton, Bild) | `config.call.enabled` |
| Klingeln | Benachrichtigung der App `notifications` mit Objekt `call`; erneut klingeln `POST /api/v4/call/{token}/ring/{attendeeId}` | `send-call-notification` |

**Eigenheiten, auf die wir achten:**
- **Lesen ändert nichts.** Ab Werk setzt das Lesen von Nachrichten den Lesestand und löscht
  Benachrichtigungen. Der Kern schickt darum immer `setReadMarker=0`, auf Servern mit
  `chat-keep-notifications` auch `markNotificationsAsRead=0`, und `noStatusUpdate=1`, damit Abfragen
  den Menschen nicht „online“ zeigen. Gelesen wird nur ausdrücklich (`chat.mark_read`). „Online“ zeigt
  ausschließlich der Heartbeat (Entscheidung 10).
- **Text mit Platzhaltern:** `message` enthält `{mention-user1}`, `{file}` usw., die Werte stehen in
  `messageParameters`. Ersetzt wird mit dem vorhandenen `rich::plain`. Dateien und Erwähnungen gibt
  es zusätzlich als eigene Felder, damit das Chatfenster sie als Chips zeigen kann.
- **Systemnachrichten** („Anna started a call“) haben `messageType: system`. Das Chatfenster zeigt sie klein,
  manche gar nicht (`reaction`, `message_edited`: Sie ändern nur eine andere Nachricht). Verpasste Anrufe stehen
  als „Missed video call“ im Verlauf.
- **Antworten auf Bearbeiten und Löschen** sind selbst Systemnachrichten. Sie ersetzen die Nachricht
  im Cache und werden nicht gezeigt.
- **Lobby und schreibgeschützte Unterhaltungen** antworten mit 412 bzw. 403.
- **Föderierte Unterhaltungen** (Teilnehmer auf anderen Servern) gehen mit `federation-v1` wie
  andere. Ohne die Capability lesen wir sie nur aus der Liste der Unterhaltungen. Status zeigen föderierte
  Server nicht.
- **Tipp-Anzeige** läuft über das Signaling (`startedTyping`/`stoppedTyping`, gesteuert über `typing-privacy`).
  Der Web-Client kann sie auch mit internem Signaling, dafür braucht es eine Sitzung in der Unterhaltung und je
  offenem Chat ein Long Polling; die Mobil-Apps nur mit HPB ([chat-machbarkeit.md](chat-machbarkeit.md)). Darum
  erst später (siehe Offen); bis dahin zeigt das Chatfenster nichts statt einer falschen Anzeige.
- **Versionen:** Nextcloud unterstützt die letzten drei Hauptversionen; alle bringen Talk 18 oder
  neuer mit. Fehlt eine Capability trotzdem, fehlt die Funktion in der Oberfläche, und der Befehl antwortet
  `unsupported`.

## Entscheidungen

Punkte 1 bis 9, 11 und 12 stammen aus #201 (2026-10-15, dort 1 bis 11) und gelten weiter, angepasst an die
Fenster und die Präsenz; die übrigen sind mit #470 (2026-11-06) dazugekommen, die Stufen (18) sind neu geschnitten.

1. **Name:** App `torrochat`, Bereich `chat`, MCP-Tools `chat_*`, Fensterklasse `torro-chat`. Der
   Bereich ist neutral wie `contacts`: Talk ist der erste Anbieter, Matrix oder Slack
   könnten folgen. Die Eingangsquelle heißt weiter `talk:<login>@<host>`, sie ist eine Quelle des
   Eingangs, kein Bereich. Der Befehl aus TL2 heißt darum **`chat.send_message`** (statt
   `talk.send_message`), MCP `chat_send_message`. Der Status hat einen eigenen Bereich `presence`
   (Entscheidung 10).
2. **Crates:**
   - `torro-chat` (neu): neutrales Modell (Unterhaltung, Nachricht, Autor, Erwähnung, Datei, Person mit Status,
     Anrufzustand), Anbieter-Trait, Quelle für die App, Demo. Ohne Ein-/Ausgabe, Vorbild `torro-pim`.
   - Talk und User status kommen in das bestehende `torro-nextcloud` (`talk.rs` wächst um Nachrichten, Senden
     und Anrufzustand, `user_status.rs` neu), das Hochladen und Teilen von Dateien nach `files.rs` (neu).
   - Der Kern-Teil kommt nach `crates/torroworkplace/src/chat/` und `…/src/presence/`.
   - `torrochat` (neu) ist die App mit Kontaktliste, Chatfenstern und Anruffenstern.
3. **Rechte je Konto**, wie bei Mail: `Rights.chat_accounts: Selection` mit den Kennungen der
   Nextcloud-Konten (`nextcloud:<login>@<host>`). Fehlt in alten Rechten = nichts. Für `presence` gibt es eine eigene Auswahl
   `Rights.presence_accounts`: Wer einem Agenten nur den Status freigibt, öffnet ihm damit nicht die Chats des
   Kontos, und umgekehrt. `presence.list_people` gibt nur Kennung, Anzeigename und Status heraus, nichts weiter
   aus dem Systemadressbuch; so umgeht es `Rights::address_books` nicht.
   - **Was ein Client nicht sehen darf, gibt es für ihn nicht:** Unterhaltungen und Personen anderer Konten sind
     `not-found`, gefiltert vor dem Lesen, auch bei Zählern und Suche.
   - **Die Rechte-Vorlagen öffnen kein Konto.** Sie setzen nur die Stufe in `chat` und `presence`;
     `chat_accounts` und `presence_accounts` bleiben leer (wie bei Kontakten und Notizen). Beim Verbinden eines Agenten sind die Konten mit
     Talk vorgewählt, und der Mensch bestätigt sie. `Rights::template()` vergleicht beide nicht mit.
   - **Eingangsquelle und Chat sind getrennt.** `inbox_sources: talk:<x>` zeigt die Einträge im
     Eingang (Titel, letzte Zeile). Den Verlauf lesen und antworten braucht `chat` und das Konto in
     `chat_accounts`. Ein Client, der nur den Eingang sieht, bekommt im Paneel kein Antwortfeld.
   - Mit einem Konto sieht ein Client alle Unterhaltungen darin, auch private 1:1-Chats. Wer das
     nicht will, gibt das Konto nicht frei. Eine Auswahl je Unterhaltung gibt es bewusst nicht: Neue
     Unterhaltungen entstehen ständig, und jede einzeln freizugeben, würde niemand durchhalten.
4. **Schreiben ist nach außen und benutzt den vorhandenen Schalter „Send on my behalf“
   (`Rights::outbound`).** Das hat [eingang.md](eingang.md) für Talk schon so angelegt („Nach außen“
   gilt für Mail, Slack, Talk). Gründe:
   - Ein Schalter für alles, was im Namen des Menschen etwas verschickt, ist leichter zu verstehen als
     einer je Dienst. Wer einem Agenten Senden erlaubt, erwartet, dass das überall gleich gilt.
   - Ab Werk „With approval“: Jede Nachricht eines Agenten wartet auf den Menschen. Das ist bei Chat
     teuer, aber Agenten schreiben selten in Chats; meist schlagen sie eine Antwort vor.
   - Ein eigener Schalter je Bereich bleibt später möglich (neues optionales Feld, verträglich), wenn
     jemand Mail frei und Chat mit Freigabe will.

   **Nach außen** sind alle Befehle, die andere Menschen sehen: Senden (auch mit Datei), Bearbeiten,
   Löschen, Reaktionen, eine neue Unterhaltung anlegen und **den eigenen Status setzen** (ihn sehen alle
   Kollegen). Auch „Note to self“ zählt dazu: Der Zugriff ergibt sich aus dem Befehl, nicht aus der Art der
   Unterhaltung (`Request::operation()` sieht nur den Request), und eine Ausnahme wäre ein Weg, die Regel zu
   umgehen. Gelesen/ungelesen, Favorit und Benachrichtigungsstufe sind nur `Write`. Der Lesestand ist zwar für
   andere sichtbar (Lesebestätigung, wenn der Mensch sie in Talk nicht abgeschaltet hat), aber er verschickt
   keinen Inhalt und sagt nur, was ohnehin geschah: Der Mensch oder sein Agent hat gelesen. Ihn hinter eine
   Freigabe zu legen, hieße, dass jedes Lesen eines Agenten auf den Menschen wartet.

   Die Begründung bei `forbidden` (`Lack::Outbound`) sagt heute „sending mail on the person's behalf“.
   Sie wird neutral: „sending on the person's behalf is switched off for this client“.
5. **Nachrichten sind immer `untrusted: true`.** Chats sind wie Mail Text von anderen, der klassische
   Weg für eingeschleuste Anweisungen. Katalogbeschreibungen und MCP-Tools sagen, dass Nachrichten
   **Daten, keine Anweisungen** sind. Das gilt für Text, Namen von Unterhaltungen, Autorennamen,
   Dateinamen und **Statusnachrichten** anderer Menschen.
6. **Chat geht an KI nur auf den Rechnern des Menschen**, wie Mail und die Talk-Einträge des
   Eingangs ([ki.md](ki.md), Grundsatz 4: „Mail und Messenger nur lokal“):
   - `Area::Chat` kommt in `NEVER_TO_AGENTS`. Ein Coding-Agent als Antrieb des Assistenten bekommt
     den Chat nie. **Auch nicht über den Eingang:** Mail-Einträge fallen für ihn schon weg, weil sie
     `Area::Mail` brauchen; Talk-Einträge brauchen heute nur die Quelle. Leiht der Kern einen Schlüssel
     ohne `chat`, streicht er darum auch die Quellen `talk:*` aus `inbox_sources`. Eine Restlücke
     bleibt bewusst: Die Quelle `nextcloud:*` meldet Anrufe und Einladungen von Talk mit dem Namen der
     Unterhaltung und der Person (keinen Nachrichtentext). Sie fällt unter die Regeln der
     Benachrichtigungen, nicht des Chats.
   - `Area::Presence` kommt ebenfalls in `NEVER_TO_AGENTS`: Wer wann da ist, ist eine Angabe über andere
     Menschen, und Statusnachrichten sind fremder Text.
   - Die Datenregel des Assistenten (`assistant/data_rule.rs`) hält Antworten der `chat.*`- und
     `presence.*`-Befehle für Modelle anderswo zurück, wie `mail.*`. Gebaut als Positivliste (Vorbild
     `MAIL_ANSWERS_ALLOWED`): Durch gehen nur Antworten ohne fremden Text (`ok`, die eigene Antwortart
     `chat-sent` mit nur der Kennung der gesendeten Nachricht, Fehler); jeder spätere Befehl ist damit von selbst
     zurückgehalten.
   - Wer `chat.*` oder `presence.*` aufgerufen hat, hat fremden Text gelesen, wie bei Mail: `reads_mail` und
     `may_have_read_untrusted` kennen `chat.`, `presence.` bzw. `Area::Chat`, `Area::Presence`. So geht auch der
     Verlauf eines Gesprächs danach nicht an ein Modell anderswo, und eine Notiz des Assistenten wird `local_only`.
   - `Rights::assistant()` bekommt `chat` und `presence` nicht. Der Mensch schaltet sie in „What the assistant may
     use“ ein und wählt die Konten.

   Externe Agenten mit eigenem Schlüssel (Claude Code über MCP) sehen Chat und Status, wenn der Mensch es
   ihnen gibt; das ist dieselbe Grenze wie bei Mail.
7. **Kein Verlauf auf der Platte.** Der Kern hält Nachrichten nur im Speicher: die letzten 200 je
   Unterhaltung, die in den letzten 30 Minuten gelesen wurde, höchstens 20 Unterhaltungen. Talk ist
   ein Online-Dienst, der Verlauf liegt auf dem Server, und ein zweites Archiv privater Chats auf dem
   Rechner wäre ein unnötiges Risiko. Ohne Netz zeigt das Chatfenster, was im Speicher ist, und sagt
   „Offline“. Auch der Status anderer liegt nur im Speicher.
8. **Kein Senden später.** Ohne Netz antwortet `chat.send_message` mit `offline`, und der Text bleibt
   im Eingabefeld. Eine Warteschlange wie bei Notizen gibt es nicht: Eine Chatnachricht, die Stunden
   später ankommt, passt meist nicht mehr zum Gespräch.
9. **Live ohne eigenen Dauerbetrieb.**
   - Die Liste der Unterhaltungen holt weiter die Schleife des Nextcloud-Kontos alle 60 s
     (`sources/nextcloud.rs`, E5.9). Sie gibt die Liste an den Chat-Cache weiter, eine Abfrage für
     beide. Fragt eine App `chat.list_conversations` und ist die Liste älter als 10 s, holt der Kern
     sie im Hintergrund neu.
   - **Long Polling nur für Unterhaltungen, die gerade jemand ansieht:** `chat.get_messages` mit
     `after` **vom App-Schlüssel** merkt die Unterhaltung für 90 s vor. Für vorgemerkte Unterhaltungen
     hält der Kern eine Long-Polling-Anfrage offen, höchstens drei zugleich (offene Chatfenster zuerst, das
     vorderste vor den anderen). Apps fragen den Kern alle 2 s mit `after`; das ist ein Blick in den Speicher.
     Clients merken nichts vor: Sie bekommen den Cache, und ist er älter als 10 s, holt der Kern einmal neu, je
     Client höchstens eine solche Abfrage zur Zeit (sonst nur der Cache). So kann kein Agent die Plätze der
     Oberfläche belegen oder den Server mit Abfragen über viele Unterhaltungen beschäftigen.
   - **Klingeln** erkennt der Kern über die Benachrichtigungen des Kontos (Objekt `call`) und über `hasCall` in
     der Liste. Damit ein Anruf nicht bis zu 60 s unbemerkt bleibt, fragt der Kern die Benachrichtigungen alle
     10 s ab, solange die Kontaktliste oder ein Chatfenster offen ist, und nutzt `notify_push`, wo es läuft. Wie
     schnell das wirklich ist, misst die erste Aufgabe der Stufe 5.
   - **Status** holt der Kern im Takt der Nextcloud-Schleife (60 s), nur solange die Kontaktliste offen ist, sonst
     gar nicht ([praesenz.md](praesenz.md), „Idee für den Aufbau“, Punkt 2).
10. **Präsenz ist ein eigener Bereich `presence`**, kein Teil von `chat`: Den Status gibt es in Nextcloud auch
    ohne Talk (App User status), und wer einem Agenten Chats gibt, gibt ihm nicht von selbst, wer wann am Rechner
    sitzt.
    - **Wer in der Liste steht:** die Menschen aus dem Systemadressbuch des Kontos. Ist es gesperrt, die
      Menschen, mit denen es schon eine 1:1-Unterhaltung gibt, und wen der Mensch über die Suche
      (`chat.find_people`) hinzufügt (`presence.add_person`, gespeichert im Kern). Bleibt die Liste leer, sagt sie warum („Your Nextcloud does not show other
      users“).
    - **Abfragen:** Liefert `/statuses` etwas, reicht ein Aufruf (seitenweise). Ist die Liste leer, weil der Admin
      beschränkt hat, fragt der Kern einzeln (`/statuses/{userId}`), höchstens 50 Menschen je Minute; die übrigen
      zeigen „Status unknown“ statt „Offline“.
    - **Heartbeat ab Werk aus.** Der Schalter „Show others when I am online“ (Steuerzentrale und Einstellungen
      des Chats) schaltet ihn ein. Dann schickt der Dienst alle 5 Minuten `online` oder `away` nach der
      Leerlaufzeit des Systems, bei gesperrtem Bildschirm und im Ruhezustand nichts. Aus, weil „ich bin online“
      etwas über den Menschen verrät. Ohne Heartbeat zeigt die Liste unten „You appear offline“ mit dem
      Schalter daneben.
    - **Der eigene Status** (online, abwesend, nicht stören, unsichtbar, Nachricht mit Ablauf) steht unten in der
      Kontaktliste und im Tray-Menü. Setzen ist nach außen (Entscheidung 4). „Do not disturb“ stellt auch das
      Klingeln in der Suite ab.
    - **Mehrere Konten:** je Nextcloud-Konto eine Gruppe in der Liste, ab zwei Konten mit Kontonamen.
11. **Schnell antworten im Notification Hub** (Stufe 1), nicht im Tray. Das Tray zählt weiter nur
    bei „Inbox (3)“ und listet keine einzelnen Meldungen (Regel aus E3.3b). Die Benachrichtigung eines
    Talk-Eintrags bekommt den Knopf „Reply“. Er öffnet den Notification Hub beim Eintrag mit dem
    Cursor im Antwortfeld; ab Stufe 3 öffnet „Open chat“ das Chatfenster. Eine Antwort direkt in der
    Systembenachrichtigung braucht nativen Code je System und ist offen (siehe unten).
12. **Teilen in einen Chat (TL2)** gehört zum Chat, Stufe 4. `chat.send_message` bekommt
    `items` und `fields` wie `mail.save_draft` in TL1. Der Kern baut die Karte über `share::card_for`
    mit den Rechten des Aufrufers, lädt die Datei in den Anhangsordner und teilt sie mit dem Text der
    Karte als Beschriftung. Mehr unter [Teilen in einen Chat](#teilen-in-einen-chat-stufe-4).
13. **Anrufe werden selbst gebaut** (entschieden vom Menschen am 2026-11-06, nach
    [chat-machbarkeit.md](chat-machbarkeit.md); empfohlen war, sie zuerst im Browser zu öffnen). Ein eigenes
    Anruffenster im Torro-Design auf Linux, macOS und Windows, mit Googles libwebrtc über die Crates `libwebrtc` und
    `webrtc-sys` von LiveKit (ohne LiveKit-Server), eigenem Talk-Signaling und dem Bild in Iced. Zuerst nur Anrufe zu
    zweit auf Servern mit internem Signaling (wie dem eigenen); HPB und Gruppenanrufe später. Stufe 5 beginnt mit
    einem Versuch als eigenem Issue (Abschnitt [Anrufe](#anrufe-stufe-5)). Bis das Anruffenster auf einem System
    läuft, öffnet „Accept“ den Anruf im Browser (`<server>/call/<token>`); „Open in Talk“ bleibt danach im Menü.
14. **Agenten rufen nicht an.** Anruf starten, annehmen, ablehnen und Kamera oder Mikrofon einschalten gibt es nur
    mit dem App-Schlüssel, also nur durch einen Menschen in der Oberfläche, wie beim Mikrofon des Assistenten
    ([api.md](api.md), „Was nie nach außen geht“, Punkt 6). Sonst könnte ein Agent im Namen des Menschen
    jemanden anrufen oder unbemerkt mithören und mitsehen. Agenten sehen höchstens, ob in einer Unterhaltung ein
    Anruf läuft (`chat.get_call`, `Read`). Bild und Ton gehen auf keinem Weg über API oder MCP. Das ist ein
    **vorgeschlagener neuer Punkt 11** für „Was nie nach außen geht“ (Wortlaut unter [Befehle](#befehle)); `api.md`
    nimmt ihn mit der ersten Aufgabe der Stufe 5 auf. Ablehnen und Auflegen dürfen Agenten, weil beides nichts
    einschaltet.
15. **Fenster wie ICQ.** Kontaktliste und Chat sind zwei getrennte Fenster, keine Ansicht der Kontakte-App und
    kein Fenster mit Liste und Verlauf nebeneinander (so noch in #201). Je Unterhaltung höchstens ein
    Chatfenster; ein zweiter Doppelklick holt das offene nach vorn.
16. **Chat- und Anruffenster schweben.** Fenster-Manager mit Kacheln ordnen sonst jedes neue Fenster ein, und
    jeder Doppelklick auf einen Namen teilte den Bildschirm neu auf. Die Kontaktliste selbst ist eine normale
    Kachel (sie läuft dauerhaft, oft in einer Workspace-Vorlage), Chat-, Klingel-, Anruf- und Gerätefenster
    schweben. Regeln je Fenster-Manager unter [Fenster](#fenster).
17. **Gruppenchats: drei Fassungen, offen bis zur Wahl** beim Durchsehen der Entwürfe: (A) ein Abschnitt
    „Groups“ unter den Personen, (B) ein Umschalter „People | Groups“ oben in der Liste, (C) ein Knopf „Group
    chats…“ unten, damit die Liste rein aus Personen besteht. Ein Gruppenchat öffnet sich in jeder Fassung im
    selben Chatfenster wie einer zu zweit. Gruppenanrufe kommen später.
18. **Stufen** (neu geschnitten mit #470, ersetzen CH1 bis CH4 aus #201):
    - **Stufe 1 · Text im Kern:** Bereich `chat`, API, MCP und schnell antworten im Notification Hub.
    - **Stufe 2 · Präsenz:** Bereich `presence`, Status anderer, eigener Status, Heartbeat, Tray-Menü.
    - **Stufe 3 · Fenster:** die App `torrochat` mit Kontaktliste und Chatfenstern, Schweben je Fenster-Manager,
      neue Unterhaltung zu zweit.
    - **Stufe 4 · Teilen in einen Chat** (TL2 aus [teilen.md](teilen.md)).
    - **Stufe 5 · Anrufe zu zweit**, selbst gebaut: (a) Versuch, (b) Klingeln, Annehmen und Ablehnen in der Suite
      (Anruf zunächst im Browser), (c) Anruffenster und Gerätewahl.
    - **Stufe 6 · Pflege:** eigene Nachrichten bearbeiten und löschen, Reaktionen, Favoriten,
      Benachrichtigungsstufe, Suche in Nachrichten.

    Jede Stufe ist für sich nutzbar und über alle drei Wege erreichbar, soweit Entscheidung 14 nichts anderes
    sagt.

## Aufbau

```
torroinbox (Paneel, Stufe 1) ───────┐
torrochat  (Kontaktliste, Chat-   ──┼─ IPC ─▶ torroworkplace (Kern)
            fenster, Anruffenster)  │          ├─ chat/service.rs   Befehle, Rechte-Filter, Freigabe-Fakten, Drossel
torrocontacts (Teilen) ─────────────┘          ├─ chat/cache.rs     Unterhaltungen und Nachrichten im Speicher
   │                                           ├─ chat/live.rs      Long Polling der vorgemerkten Unterhaltungen, Klingeln
   │ Medien (Stufe 5):                         ├─ chat/call.rs      Sitzung, Anruf beitreten/verlassen, Signaling (Stufe 5)
   │ libwebrtc, Kamera, Ton,                   ├─ chat/share.rs     Karte → Datei hochladen → teilen (Stufe 4)
   │ Bild in Iced                              ├─ presence/         Status anderer, eigener Status, Heartbeat (Stufe 2)
   │ (nur im Anruffenster)                     └─ sources/nextcloud.rs, sources/talk.rs  Liste alle 60 s (gibt es)
   ▼                                                    │
 Gegenstelle (Browser, Talk-App)        torro_chat::ChatProvider (Trait, Modell)
   ◀──── WebRTC (Bild, Ton), STUN/TURN ────▶ └─ torro-nextcloud: talk.rs, user_status.rs, files.rs
```

Der Kern spricht mit Talk (Anmeldung, Sitzung, Signaling), die App nur mit dem Kern. Bild und Ton laufen dagegen
direkt zwischen dem Anruffenster und der Gegenstelle: Kamera und Mikrofon gehören dem Fenster, in dem man sie
sieht, und macOS fragt die Erlaubnis für genau dieses Programm ab.

**Kennungen:**
- **Konto:** die Kennung des Nextcloud-Kontos, `nextcloud:anna@cloud.example`.
- **Unterhaltung:** `<konto-id>/<token>`, zum Beispiel `nextcloud:anna@cloud.example/x7k2m9qa`.
  Getrennt am letzten `/`; der Token muss `RoomToken::parse` bestehen.
- **Nachricht:** `<unterhaltung>/<nachricht-id>`, zum Beispiel
  `nextcloud:anna@cloud.example/x7k2m9qa/5122`.
- **Person:** `<konto-id>/<user-id>`, zum Beispiel `nextcloud:anna@cloud.example/ben`. Die Kennung kommt aus dem
  Systemadressbuch oder aus Talk, nie aus einem Anzeigenamen.
- Ein Eintrag der Quelle `talk:<x>` im Eingang führt seine Unterhaltung als neues Feld `conversation`.

**Fehler** (feste Codes):

| Fall | Code |
|---|---|
| Server nicht erreichbar | `offline` |
| 401 | `auth`, danach Remote-Wipe-Prüfung wie bei CalDAV |
| Talk fehlt auf dem Server | `not-connected` (Präsenz geht trotzdem, wenn User status da ist) |
| Unterhaltung weg, Mensch nicht mehr Teilnehmer, fremdes Konto | `not-found` |
| schreibgeschützt (403), Lobby (412), Funktion fehlt (Capability) | `unsupported` mit Grund („This conversation is read-only.“) |
| Text zu lang (413 oder über `max-length`) | `error` |
| Bearbeiten nach 24 h, Löschen nach 6 h (400) | `unsupported` („Messages can only be edited for 24 hours.“) |
| 429, Drossel des Kerns | `busy` |

`message` ist immer ein fester englischer Text des Kerns. Meldungen aus der Antwort von Talk landen nie
roh darin: Sie sind fremder Text und kämen sonst ungeprüft zu einem Agenten.

## Datenschutz und Sicherheit

- **Kein Inhalt im Protokoll:** Befehl, Konto und Prüfsummen der Kennungen (`audit::ids`). Texte,
  Namen, Statusnachrichten, Tokens und Suchbegriffe stehen dort nie. **Gerätenamen** (Kamera, Mikrofon,
  Lautsprecher) auch nicht: Sie verraten oft Name und Rechner des Menschen („Annas AirPods Pro“).
- **Freigaben** zeigen aus dem Cache des Kerns, nachgeschlagen über die Kennung (`approvals::Facts`):
  - den Namen der Unterhaltung und ihre Art („Direct chat with Anna Example“, „Group: Team Alpha“),
    bei öffentlichen Unterhaltungen „Public conversation – anyone with the link can read it“, bei
    föderierten „Includes people on other servers“;
  - den Text, der wirklich geschickt wird, entschärft (keine Steuerzeichen). Die Zusammenfassung
    (Benachrichtigung, Eintrag) zeigt höchstens 1.000 Zeichen und danach „… ({n} more characters)“;
    die Freigabe in der Steuerzentrale und im Notification Hub zeigt mit „Show all“ den ganzen Text, damit
    niemand Ungesehenes bestätigt;
  - bei einer Antwort den Autor der zitierten Nachricht;
  - „Notifies everyone in the conversation“, wenn der Text `@all` erwähnt, „Sent silently“ bei
    `silent`;
  - bei Bearbeiten „alt → neu“, bei Löschen den Anfang der Nachricht, bei Reaktionen das Emoji; „alt“ und der
    Anfang kommen aus dem Cache bzw. frisch vom Server über die Kennung, nie aus dem Request des Clients, und fehlt
    die Nachricht, steht dort „unknown message“;
  - beim eigenen Status „Everyone on <server> sees: Do not disturb · In a meeting until 11:00“;
  - bei Karten (Stufe 4) jede Karte wie bei Mail und dass eine Datei in den Talk-Ordner geladen wird.

  Namen gehen durch dieselbe Entschärfung wie in anderen Freigaben (eine Zeile, 60 Zeichen). Kennt
  der Cache eine Unterhaltung nicht mehr, steht dort „unknown conversation“.
- **Wartende Freigaben auf der Platte:** Bis der Mensch entscheidet, liegt der Request mit dem Text in
  der Datei der Freigaben (0600). Das ist die einzige Stelle, an der Chat-Text auf die Platte kommt
  (Entscheidung 7). Heute hält der Kern entschiedene und abgelaufene Freigaben noch einen Tag samt
  Request; bei `chat.*` und `presence.set_own_status` leert er den Text darum sofort bei der Entscheidung oder
  dem Ablauf.
- **Drossel:** Ein Client (gezählt je Client-Kennung, damit geliehene Schlüssel des Assistenten sie
  nicht umgehen) führt höchstens 30 Nach-außen-Befehle in `chat` und `presence` je Stunde und 5 je Minute aus
  (Senden, Bearbeiten, Löschen, Reaktionen, neue Unterhaltung, eigener Status zusammen), sonst `busy`. Auch mit
  „Free“ kann ein entgleister Agent so niemanden zuschütten. Der App-Schlüssel ist nicht gedrosselt.
- **Erwähnungen von allen** (`@all`) sind für Clients nicht verboten, stehen aber in der Freigabe.
- **Links und Dateien:** Dateien in Nachrichten zeigt das Chatfenster mit Name, Größe und „Open in
  Nextcloud“. Der Link wird aus der Kennung der Datei auf dem Ursprung des Kontos gebaut
  (`<server>/f/<id>`), nie aus der Antwort übernommen (Regel 15 aus E5). Links im Text öffnen sich nur
  nach einem Klick und nur mit `https`, `mailto`. Bilder lädt der Verlauf nicht (kein Verfolgen,
  nur Text).
- **Konto weg oder Remote Wipe:** Cache und Status des Kontos werden geleert, laufende Long-Polling-Anfragen
  und Anrufe werden beendet, das Konto fällt aus allen `chat_accounts`.
- **Anrufe** (ergänzt mit #470):
  - **Kein Mitschnitt.** Die Suite nimmt weder Bild noch Ton auf, auch nicht als Puffer auf der Platte, und
    bietet die Aufnahme von Talk (Recording-Server) nicht an.
  - **Bild und Ton gehen nie an eine KI** und nie über API oder MCP, auch nicht als Standbild oder Transkript.
    Eine Zusammenfassung eines Anrufs ist nicht Teil dieses Konzepts.
  - **Die Vorschau der Kamera bleibt lokal.** Sie läuft im Fenster der Gerätewahl, wird nicht verschickt und
    nicht gespeichert; die Kamera geht aus, sobald das Fenster zu ist.
  - **Zugangsdaten für TURN sind Geheimnisse.** Talk gibt sie über `signaling/settings` heraus, 24 h gültig und je
    Sitzung; sie öffnen nur das TURN-Relay, nicht das Konto. Sie gehen an keinen Client, in kein MCP-Tool, in kein
    Protokoll und in keine Freigabe. libwebrtc im Anruffenster braucht sie aber selbst, und Punkt 1 von „Was nie nach
    außen geht“ sagt heute „auch nicht über den App-Schlüssel“. **Vorschlag, offen bis zur Zustimmung des Menschen
    in der ersten Aufgabe der Stufe 5:** ein einmaliger Never-Befehl `chat.call_credentials { call }`, gültig nur für
    den laufenden Anruf und nur mit der Kennung `call`, die allein das Fenster des Anrufs kennt; als eigener Typ
    (etwa `TurnCredential`, ohne `Debug`, eigene Angabe im Schema, nicht `Secret`, das nur für Eingaben gedacht ist);
    das Fenster hält sie nur im Speicher. Statt über IPC kann der Kern das Anruffenster auch selbst starten und sie
    über stdin geben. Beides ist eine Ausnahme in Punkt 1, die `api.md` im selben Schritt nennt. Ohne Zustimmung
    bleibt nur ein Anruf ohne TURN (scheitert hinter strenger NAT) oder der Browser.
  - **Die Sitzung bei Talk hält der Kern.** Für den Anruf tritt er der Unterhaltung mit einer Cookie-Sitzung bei;
    Cookie und App-Passwort bleiben im Dienst.
  - **Kamera und Mikrofon an zeigt das Fenster immer**, mit Icon und Wort, nie nur mit Farbe. Ein Anruf ohne
    sichtbares Fenster gibt es nicht; wird das Anruffenster geschlossen, endet der Anruf.
  - **Die Wahl der Geräte** speichert der Kern als Einstellung je Rechner (`chat_camera`, `chat_microphone`,
    `chat_speaker` mit der Kennung des Geräts), nicht im Protokoll und nicht in Freigaben.

## Fenster

Entwürfe: [prototypes/chat/](prototypes/chat/index.html) (alle Fenster in hell und dunkel, Fassungen
umschaltbar). Alle Fenster gehören zur App `torrochat` und folgen dem Torro-Design
([`torro-design`](../.claude/skills/torro-design/SKILL.md)): Inter, Tokens des aktiven Designs, Status nie nur über
Farbe, Rot nur für den einen Primärknopf.

**Kontaktliste** (Fensterklasse `torro-chat`, Kachel, schmal, 300 breit gedacht):
- **Obere Zeile:** „Contacts“, rechts die Einstellungen (`shell::SETTINGS_ICON`).
- **Suchfeld** filtert Namen und Statusnachrichten in der Liste; Enter öffnet den ersten Treffer. Ohne Treffer
  „Find on server…“ (`chat.find_people`).
- **Gruppen nach Status:** „Online“, „Away“, „Do not disturb“, „Offline“ in dieser Reihenfolge, jede mit Zahl und
  einklappbar; „Offline“ ab Werk zugeklappt, sobald mehr als 20 Menschen darin stehen. In jeder Gruppe nach Name.
  Jede Zeile: Avatar mit Status-Marke (jede Marke mit eigener Form: Punkt, Mond, Minus, Ring), Name, darunter die
  Statusnachricht oder „Last seen yesterday“, rechts die Zahl ungelesener Nachrichten.
- **Gruppenchats** je nach gewählter Fassung (Entscheidung 17).
- **Fuß:** eigener Status mit Avatar und Nachricht, ein Klick öffnet ein Menü mit den Status, „Set status
  message…“ (mit Ablauf: 30 min, 1 h, heute, diese Woche) und „Show others when I am online“.
- **Bedienung:** Pfeiltasten wandern, Enter oder Doppelklick öffnet das Chatfenster, `⌘F`/`Ctrl+F` springt in die
  Suche. Kontextmenü: „Open chat“, „Audio call“, „Video call“, „Show contact“ (öffnet `torrocontacts` beim Kontakt mit
  derselben Mail-Adresse bzw. Nextcloud-Kennung; die Zuordnung macht der Kern, ohne Treffer fehlt der Eintrag),
  „Remove from list“ (nur für selbst hinzugefügte).
- **Größen:** Mini zeigt die Zahl der Menschen online und die ungelesenen Unterhaltungen; Leiste die Avatare der
  Menschen online. Breiter als schmal wird die Liste nicht gestreckt (`size::content_max()`).
- **Leer:** ohne Konto „Connect Nextcloud“ (öffnet die Steuerzentrale), ohne User status „Your Nextcloud does not
  share status“, ohne sichtbare Menschen „Your Nextcloud does not show other users“ mit der Suche.

**Chatfenster** (Klasse `torro-chat-conversation`, schwebend, 440 × 680 gedacht, Größe und Ort je Unterhaltung
gemerkt):
- **Kopf:** Avatar mit Status, Name, darunter Status und Statusnachricht (bei Gruppen die Zahl der Teilnehmer),
  rechts „Audio call“, „Video call“ und „…“ (Mark as unread, Notifications, Open in Talk, Show contact).
- **Verlauf:** Nachrichten nach Tag gruppiert, eigene rechts. Zitat bei Antworten, Erwähnungen hervorgehoben,
  Dateien als Zeile mit Symbol, Name, Größe und „Open“. Einfaches Markdown (fett, kursiv, Code, Links), keine
  Bilder. Trenner „New“ beim Lesestand. Systemnachrichten klein in `TEXT_3`, verpasste Anrufe mit Icon. Nach oben
  scrollen lädt ältere.
- **Eingabe:** mehrzeilig, Enter sendet, Shift+Enter bricht um. `@` öffnet Vorschläge. Ein Zähler erscheint ab
  90 % der Höchstlänge. Im Menü „Send without notification“.
- **Gelesen:** Ist das Fenster im Vordergrund und die neueste Nachricht sichtbar, setzt die App den Lesestand.
- **Tasten:** `Esc` bricht eine Antwort ab bzw. schließt das Fenster, `⌘⇧A`/`Ctrl+Shift+A` startet einen
  Audioanruf, `⌘⇧V`/`Ctrl+Shift+V` einen Videoanruf.
- **Leer und Fehler:** Offline zeigt den Speicher mit „Offline“ im Kopf; eine verschwundene Unterhaltung „This
  conversation is no longer available“.

**Eingehender Anruf:** zwei Fassungen zur Wahl, siehe Entwürfe: eine **Benachrichtigung** oben rechts (Decline,
Audio only, Accept) oder ein **kleines Fenster** mittig (Klasse `torro-chat-ring`, schwebend, 340 × 360). „Accept“
ist der Primärknopf. Bei „Do not disturb“ klingelt nichts; der Anruf steht danach als verpasst im Chat und im
Notification Hub. Ein Klingeln endet nach 45 s oder wenn die Gegenseite auflegt.

**Anruffenster** (Klasse `torro-chat-call`, schwebend, frei in der Größe, 760 × 500 gedacht): Fassung A Bild in
Bild (die andere Person groß, das eigene Bild klein in einer Ecke, verschiebbar) oder B nebeneinander. Unten
Mikrofon (`M`), Kamera (`V`), Geräte, Bildschirm teilen (ausgegraut, später) und „Leave call“ als Primärknopf.
Oben Dauer und „Show chat“. Schließen beendet den Anruf.

**Gerätewahl** (Klasse `torro-chat-devices`, schwebend, 420 × 660): Kamera mit Vorschau („Preview stays on this
computer“), Mikrofon mit Pegel, Lautsprecher mit „Play test sound“, Schalter „Join calls with camera off“, „Done“.
Ohne Gerät oder ohne Recht dazu steht der Weg zur Systemeinstellung da. Ein neu eingestecktes Gerät erscheint
sofort in der Liste, gewechselt wird nur hier.

**Schweben je Fenster-Manager** (Entscheidung 16): Der Kern setzt die Regeln für die festen Klassen des Chats
beim Start selbst über `torro-wm`, wie heute für `WORKPLACE_DIALOG_CLASS`; einen Request dafür gibt es nicht:
- **Hyprland:** `float_dialogs` gibt es schon (Regel `float = true, center = true` je Klasse, mit dem Neuladen der
  Konfiguration erneut gesetzt); für Chatfenster ohne `center`, damit der gemerkte Ort gilt.
- **AeroSpace:** Laufzeitregeln gibt es nicht. Der Kern stellt ein neues Fenster der Klassen über
  `aerospace layout floating --window-id <id>` auf schwebend, sobald es erscheint; der Installer kann zusätzlich
  einen Block `on-window-detected` mit `layout floating` vorschlagen.
- **GlazeWM:** ebenso über den Befehl `set-floating` am neuen Fenster, alternativ eine `window_rules`-Regel in der
  Konfiguration.
- **Ohne Kachel-Manager** (macOS ohne AeroSpace, Windows ohne GlazeWM, GNOME, KDE) schweben die Fenster ohnehin.

**Notification Hub (Stufe 1)**, Paneel eines Talk-Eintrags:
- **Oben** wie heute Titel (Unterhaltung) und Quelle.
- **Verlauf:** die letzten 20 Nachrichten, die ungelesenen hinter einem Trenner „New“. Ältere über
  „Show earlier“. Systemnachrichten klein und grau.
- **Antwortfeld unten:** Enter sendet, Shift+Enter bricht um. „Reply“ an einer Nachricht zitiert sie
  über dem Feld. Nach dem Senden markiert das Paneel den Eintrag als erledigt (`inbox.set_item_done`,
  das setzt in Talk schon heute den Lesestand).
- **Ohne Recht auf `chat`** steht statt Verlauf und Feld nur die letzte Zeile und „Open in Talk“.
- **„Open chat“** (ab Stufe 3) öffnet das Chatfenster der Unterhaltung, sonst „Open in Talk“ im Browser.
- **Geöffnet** wird das Paneel beim Eintrag über `inbox.open_item`, wie heute bei Mail.
- **Benachrichtigung:** Knopf „Reply“ neben „Done“ und „Snooze 1 h“.

**Steuerzentrale:** Rechte-Zeilen „Chat (Talk)“ und „Presence“ mit je eigener Kontoauswahl (der Zusatz „Talk“
unterscheidet sie vom Assistenten); der Hinweis an „Send on my behalf“
nennt künftig Mail, Antworten auf Einladungen, Chat und den eigenen Status. Auf der Kontoseite stehen Talk und User
status bei den Diensten. Unter General der Schalter „Show others when I am online“.

**Teilen-Blatt (Stufe 4):** Ziel „Chat“ mit den letzten Unterhaltungen und Suche, siehe unten.

**Demo:** `torrochat --demo` mit sechs Menschen in allen Status, zwei Gruppen, drei Unterhaltungen mit Erwähnung,
Datei, Systemnachricht und verpasstem Anruf; `--open contacts | chat | ring | call | devices` öffnet ein Fenster
direkt (für die Sichtprüfung).

## Befehle

Geprüft mit dem Agenten `torro-api-review` (2026-11-06); seine Funde sind eingearbeitet. Die Namen der Rust-Fälle
beginnen mit `Chat…` bzw. `Presence…` (`ChatSendMessage`), damit sie nicht mit den Requests des Assistenten
(`OpenChatWindow`, KI-Platz `chat`) verwechselt werden; die Rechte-Zeile heißt „Chat (Talk)“.

Bereich `chat`:

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| 1 | `chat.list_conversations { account?, unread_only?, query?, limit? }` (Name, Art, ungelesen, Erwähnung, letzte Nachricht, läuft ein Anruf, klingelt es) | Read | `chat_list_conversations` |
| 1 | `chat.get_messages { conversation, before?, after?, limit? }` (höchstens 100, ändert keinen Lesestand) | Read | `chat_get_messages` |
| 1 | `chat.send_message { conversation, text, reply_to?, silent? }` | Write + **nach außen** | `chat_send_message` |
| 1 | `chat.mark_read { conversation, up_to? }` | Write | `chat_mark_read` |
| 1 | `inbox.get_item` mit dem neuen Feld `conversation` bei Talk-Einträgen (Erweiterung) | Read (`inbox`) | `inbox_get_item` (vorhanden) |
| 1 | `inbox.open_item` öffnet bei Talk-Einträgen den Notification Hub beim Eintrag mit dem Antwortfeld (Erweiterung, wie heute bei Mail) | wie heute | `inbox_open_item` (vorhanden) |
| 2 | siehe Bereich `presence` | | |
| 3 | `chat.mark_unread { conversation }` | Write | `chat_mark_unread` |
| 3 | `chat.suggest_mentions { conversation, query, limit? }` | Read | `chat_suggest_mentions` |
| 3 | `chat.find_people { account, query, limit? }` (Personen des Servers für eine neue Unterhaltung und die Kontaktliste) | Read | `chat_find_people` |
| 3 | `chat.create_conversation { account, with }` (1:1, gibt eine vorhandene zurück) | Write + **nach außen** | `chat_create_conversation` |
| 3 | `apps.open_app` mit `target: "chat:<unterhaltung>"` bzw. `"chat:contacts"` (Chatfenster bzw. Kontaktliste; eine Unterhaltung außerhalb von `chat_accounts` ist `not-found`, Erweiterung) | Write (`apps`) | `apps_open_app` (vorhanden) |
| 3 | `apps.open_app` mit `target: "contact:<id>"` („Show contact“: `torrocontacts` beim Kontakt; braucht Read in `contacts` und das Adressbuch in `address_books`, sonst `not-found`, Erweiterung) | Write (`apps`) | `apps_open_app` (vorhanden) |
| 4 | `chat.send_message` mit `items` und `fields` (Erweiterung, TL2) | Write + **nach außen** + Read der Inhalte | `chat_send_message` (vorhanden) |
| 5 | `chat.get_call { conversation }` (läuft ein Anruf, klingelt es, seit wann, wie viele sind drin, ob der Mensch dabei ist; keine Namen, keine Geräte, kein Bild) | Read | `chat_get_call` |
| 5 | `chat.start_call { conversation, video }` (Anruffenster öffnen, beitreten, Gegenseite klingeln lassen) | Write | `Never` (Punkt 11) |
| 5 | `chat.answer_call { conversation, video }` (annehmen, mit Bild oder nur Ton) | Write | `Never` (Punkt 11) |
| 5 | `chat.decline_call { conversation }` (eigenes Klingeln beenden, die Gegenseite telefoniert weiter) | Write | `chat_decline_call` |
| 5 | `chat.leave_call { conversation }` (auflegen, schließt das Anruffenster) | Write | `chat_leave_call` |
| 5 | `chat.call_events { call, after? }` (Signaling-Nachrichten und Teilnehmer für das Anruffenster) | Read | `Never` (Punkt 11) |
| 5 | `chat.call_signal { call, messages }` (Signaling-Nachrichten des Anruffensters an Talk) | Write | `Never` (Punkt 11) |
| 5 | `chat.call_credentials { call }` (STUN/TURN für den laufenden Anruf, einmal je Anruf; nur nach Zustimmung zur Ausnahme, siehe Anrufe) | Read | `Never` (Punkte 1 und 11) |
| 5 | `settings.set_setting` mit `chat_camera`, `chat_microphone`, `chat_speaker` (Kennung des Geräts) und `chat_join_camera_off` (Erweiterung) | Admin (`settings`); Geräte und `chat_join_camera_off: false` **nur Menschen** | `settings_set_setting` (vorhanden) |
| 6 | `chat.edit_message { id, text }` | **Delete** + **nach außen** | `chat_edit_message` |
| 6 | `chat.delete_message { id }` | Delete + **nach außen** | `chat_delete_message` |
| 6 | `chat.react { id, emoji, remove? }` | Write + **nach außen** | `chat_react` |
| 6 | `chat.set_favorite { conversation, favorite }` | Write | `chat_set_favorite` |
| 6 | `chat.set_notification_level { conversation, level }` (`default`, `always`, `mention`, `never`) | Write | `chat_set_notification_level` |
| 6 | `chat.search_messages { query, account?, limit? }` (Unified Search von Talk) | Read | `chat_search_messages` |

Bereich `presence` (Rechte: Stufe in `presence` und eigene Auswahl `presence_accounts`, Entscheidung 3):

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| 2 | `presence.list_people { account? }` (Kennung, Anzeigename, Status, Statusnachricht, Ablauf; nichts weiter aus dem Systemadressbuch; `untrusted: true`) | Read | `presence_list_people` |
| 2 | `presence.add_person { account, person }` / `presence.remove_person { account, person }` (eigene Liste, wenn das Systemadressbuch gesperrt ist; gespeichert im Kern, `presence.json` im Konfigurationsordner) | Write | `presence_add_person`, `presence_remove_person` |
| 2 | `presence.get_own_status { account }` | Read | `presence_get_own_status` |
| 2 | `presence.set_own_status { account, status, message?, icon?, clear_at? }` | Write + **nach außen** | `presence_set_own_status` |
| 2 | `settings.set_setting` mit `presence_heartbeat` (Heartbeat) | Admin (`settings`); `true` **nur Menschen** | `settings_set_setting` (vorhanden) |

Es gibt keinen Befehl für das Schweben der Fenster: Der Kern setzt die Regeln für die festen Fensterklassen des
Chats beim Start selbst, wie heute für `WORKPLACE_DIALOG_CLASS`. So kann auch kein Client fremde Klassen schweben
lassen. Größe, Ort und eingeklappte Gruppen merkt jedes Fenster in `ui.json` (`torro_gui::ui_state`), wie üblich.

**Vorgeschlagener Punkt 11 für [„Was nie nach außen geht“](api.md#was-nie-nach-außen-geht)** (`api.md` nimmt
ihn mit der ersten Aufgabe der Stufe 2 bzw. 5 auf, je nach Teil, samt Tests wie
`the_microphone_is_switched_on_by_people_only` und den Never-Befehlen in `kept_back` von `names_follow_the_rules`):

> 11. **Anrufe und Kamera:** nur mit dem App-Schlüssel, also nur durch einen Menschen im Chatfenster, beim
>     Klingeln oder im Anruffenster.
>     - **Anruf starten und annehmen** (`chat.start_call`, `chat.answer_call`): Sonst könnte ein Agent im Namen
>       des Menschen jemanden anrufen oder Mikrofon und Kamera in einen Anruf schalten.
>     - **Das Signaling eines Anrufs** (`chat.call_events`, `chat.call_signal`, `chat.call_credentials`): Wer es
>       hat, kann einem laufenden Anruf beitreten und mithören.
>     - **Geräte wählen** (`settings.set_setting` mit `chat_camera`, `chat_microphone`, `chat_speaker`) und
>       **mit Kamera beitreten** (`chat_join_camera_off: false`): Die Wahl bereitet vor, welche Kamera und welches
>       Mikrofon beim nächsten Anruf angehen. Wie beim Mikrofon entscheidet der Wert (`Request::kept_for_people`);
>       `chat_join_camera_off: true` darf jeder mit Admin in `settings`.
>     - **Medien gehen nie über den Kern:** Bild und Ton eines Anrufs, die Vorschau der Kamera, der Pegel des
>       Mikrofons und der Testton laufen nur im Anruffenster bzw. in der Gerätewahl, die dafür selbst auf die
>       Geräte zugreifen. Das ist die einzige Ausnahme von „Oberflächen lesen und schreiben nichts selbst“: macOS
>       fragt die Erlaubnis für das Programm ab, das die Kamera nutzt, und Bild und Ton durch den Dienst zu leiten,
>       kostete Zeit und hieße, dass der Dienst sie sieht. Kamera und Mikrofon an und aus schaltet das Fenster selbst
>       (keine API); den Zustand erfahren die anderen nur über `call_signal`.
>
>     Ebenso **den Heartbeat der Präsenz einschalten** (`settings.set_setting` mit `presence_heartbeat: true`): Er
>     verrät dauerhaft, wann der Mensch am Rechner sitzt. Ausschalten darf jeder mit Admin in `settings`.

Erläuterungen:
- **Durchgesetzt** werden die Never-Befehle über `Mcp::Never` in `Rights::allows` (ein Client bekommt
  `forbidden`, auch mit Admin-Rechten), die gesperrten Werte von `settings.set_setting` über
  `Request::kept_for_people`.
- **`decline_call` und `leave_call` sind für Clients offen**, weil beides nichts preisgibt und nichts einschaltet:
  Ablehnen beendet nur das eigene Klingeln, Auflegen schließt das Anruffenster (ein Anruf ohne sichtbares Fenster
  gibt es nicht). Ein Agent soll einen vergessenen Anruf beenden können, wie er `voice_enabled: false` setzen darf.
  Beides ist `Write` ohne nach außen: Die anderen sehen nur, dass der Mensch nicht da ist bzw. geht.
- **`get_call` ist `Read`** und nennt keine Namen, nur die Zahl der Teilnehmer; die Namen stehen ohnehin in der
  Unterhaltung, die `chat` zeigt. Klingeln sieht die App über `list_conversations` und `get_call` (Feld `ringing`).
- **`call`** in `call_events`, `call_signal` und `call_credentials` ist eine Kennung, die der Kern bei
  `start_call` bzw. `answer_call` vergibt und nur dem Fenster gibt, das den Anruf hält. Ein anderes Programm mit
  dem App-Schlüssel (`ctl`) kennt sie nicht.
- **Der Heartbeat ist nach außen**, aber kein eigener Befehl: Er läuft im Dienst, sobald der Mensch ihn
  eingeschaltet hat.
- **`add_person` ist nur `Write`:** Es ändert nur die eigene Liste auf diesem Rechner, niemand sonst sieht es.
- **„Nach außen“** sind `send_message`, `create_conversation`, `edit_message`, `delete_message`, `react` und
  `set_own_status`: Sie kommen in `OUTBOUND` (`torro-ipc`, `api.rs`), und der Doc-Kommentar an `Lack::Outbound`
  („always mail“) wird mit dem Text neutral (Entscheidung 4).
- **`get_messages` ist `Read`,** obwohl es eine Unterhaltung für Long Polling vormerkt: Das ändert
  nichts beim Menschen oder auf dem Server.
- **`edit_message` ist `Delete`,** weil es den alten Text unwiederbringlich ersetzt (Regel aus
  [api.md](api.md): überschreiben ist `Delete`). Bearbeiten und Löschen gehen nur bei eigenen
  Nachrichten; fremde löschen dürfen in Talk Moderatoren, das bieten wir nicht an (`unsupported`).
- **`react` mit `remove`** bleibt `Write`: Eine eigene Reaktion zurücknehmen verliert nichts.
- **`find_people`** liest das Personenverzeichnis des Servers (`/ocs/v2.php/core/autocomplete/get`
  mit `itemType=call`), höchstens 20 Treffer, mindestens zwei Zeichen. Das ist `Read` in `chat` mit
  dem Konto in `chat_accounts`. Personen und Vorschläge aus `suggest_mentions` tragen `untrusted:
  true` wie Nachrichten.
- **`create_conversation`** legt nur 1:1-Unterhaltungen an. Gruppen anlegen, Teilnehmer verwalten
  und Unterhaltungen verlassen bleiben in Talk selbst (siehe Offen).
- **`set_notification_level`** wirkt auch auf den Eingang: `rules::talk_place` richtet sich nach der
  Stufe. Es ist `Write` wie `inbox.mute_item` für eine Unterhaltung.
- **Mentions für Agenten:** Die Beschreibung von `send_message` sagt, dass Erwähnungen als
  `@"<mentionId>"` geschrieben werden und `suggest_mentions` die Kennungen liefert.

## Anrufe (Stufe 5)

Entscheidung 13: selbst gebaut. Was die Machbarkeit dazu ergab, steht in
[chat-machbarkeit.md](chat-machbarkeit.md); hier steht, wie es in die Suite passt.

**Ablauf eines Anrufs:**
1. **Starten:** „Video call“ im Chatfenster schickt `chat.start_call` (nur App-Schlüssel). Der Kern tritt der
   Unterhaltung mit einer eigenen Sitzung bei (`POST /room/{token}/participants/active`, Cookie im Speicher des
   Kerns), dann dem Anruf (`POST /call/{token}`, `flags` Ton und Bild); Talk lässt die Gegenseite klingeln. Das
   Anruffenster öffnet sich.
2. **Klingeln:** Der Kern erkennt einen eingehenden Anruf (Entscheidung 9) und meldet ihn der App, die die
   Benachrichtigung bzw. das kleine Fenster zeigt. Ist `torrochat` nicht offen, startet der Kern es.
3. **Annehmen:** `chat.answer_call` mit oder ohne Bild; der Kern vergibt die Kennung `call` und gibt sie nur dem
   Anruffenster. Ablehnen (`chat.decline_call`) beendet nur das eigene Klingeln (`notification-state`), nicht den
   Anruf der Gegenseite.
4. **Im Anruf:** Der Kern pollt `signaling/{token}` und reicht die Nachrichten (`offer`, `answer`, `candidate`,
   `mute` …) über `chat.call_events` an das Anruffenster; was das Fenster schickt, geht über `chat.call_signal`
   gebündelt zurück. Das Fenster baut mit libwebrtc je Gegenstelle eine Verbindung, zeigt das Bild und meldet Ton
   und Bild an/aus auch über den Datenkanal `status`, wie der Web-Client.
5. **Auflegen:** „Leave call“ oder Fenster schließen schickt `chat.leave_call`; der Kern verlässt Anruf und
   Sitzung. Bricht die App ab, verlässt der Kern den Anruf nach 30 s ohne Lebenszeichen von selbst.

**Bausteine** (Versionen und Gründe in der Machbarkeit):
- **WebRTC:** `libwebrtc`/`webrtc-sys` (LiveKit) mit Echo-Unterdrückung, Rauschunterdrückung und Pegelregelung
  von libwebrtc, Codecs VP8 und Opus (H.264 als zweites Angebot). Neue Abhängigkeiten nur über
  `[workspace.dependencies]`; der Bau der vorgebauten Bibliothek (clang ≥ 21 unter Linux, `-ObjC` auf macOS,
  `+crt-static` auf Windows) wird im Versuch geprüft, ebenso `scripts/ci-local.sh` ohne Netz.
- **Kamera:** eine Quelle je System, die Bilder an `NativeVideoSource` gibt. Ob `nokhwa` reicht (baut auf macOS
  gerade nicht) oder eigene Wege nötig sind, klärt der Versuch; Code mit `unsafe` geht nur nach Rückfrage und nur in
  `torro-appkit`.
- **Ton:** die Geräteverwaltung von libwebrtc (Liste, Wahl von Mikrofon und Lautsprecher).
- **Bild in Iced:** ein eigenes `shader`-Widget mit YUV-Texturen auf der wgpu von Iced, Rückfall
  `image::Handle::from_rgba`, wenn keine GPU da ist.
- **Signaling:** zuerst nur intern (der eigene Server). HPB ist ein zweites Protokoll und kommt mit den
  Gruppenanrufen. Das interne ist nicht dokumentiert; Grundlage sind der Quelltext von Talk im Stand des Servers und
  als Nachschlagewerk `nextcloud-modern-talk` (Lizenz EUPL, darum kein Code übernommen).
- **Berechtigungen:** macOS braucht `NSCameraUsageDescription` und `NSMicrophoneUsageDescription` in
  `packaging/macos/Info.plist.in` (fehlen heute) und mit Hardened Runtime die Berechtigungen für Kamera und
  Audioeingang; ohne sie beendet macOS die App beim ersten Zugriff. Windows hat einen Schalter für alle
  Desktop-Apps, Linux braucht nur unter Flatpak das Kameraportal. Fehlt die Erlaubnis, zeigt die Gerätewahl den Weg
  zur Systemeinstellung.

**Stufe 5 in drei Schritten:**
- **(a) Versuch** (eigenes Issue, Code unter `spikes/` wie M9.4): ein Anruf zu zweit zwischen einem kleinen
  Rust-Programm und Talk-Web im Browser auf dem eigenen Server, auf Linux und macOS, mit Ton in beide Richtungen ohne
  Echo und Bild. Misst Aufbauzeit, CPU-Last, Größe des Programms und prüft TURN. Ergebnis als Abschnitt in
  [research.md](research.md). Scheitert er, entscheidet der Mensch neu (Browser oder später).
- **(b) Klingeln und Annehmen in der Suite:** Benachrichtigung bzw. kleines Fenster, `get_call`, `decline_call`;
  „Accept“, „Audio call“ und „Video call“ öffnen das Gespräch zunächst im Browser (`<server>/call/<token>`). Der
  Kern tritt dann selbst keinem Anruf bei, sonst säße ein zweiter Teilnehmer ohne Bild und Ton darin; `start_call`
  und `answer_call` kommen erst mit (c).
- **(c) Anruffenster und Gerätewahl** nach den Entwürfen, mit Bild in Iced; danach öffnet „Accept“ das eigene
  Fenster.

**Grenzen der ersten Fassung:** nur zu zweit (ohne HPB rät Talk selbst ab drei Personen ab), kein Bildschirm
teilen, keine Aufnahme, keine Untertitel. Föderierte Unterhaltungen und Gäste ohne Konto erst, wenn der Versuch
sie geprüft hat.

## Teilen in einen Chat (Stufe 4)

Übernimmt TL2 aus [teilen.md](teilen.md):
- **Befehl:** `chat.send_message { conversation, text, items?, fields? }`. `items` sind
  `ShareItem`s einer Art, `fields` die Auswahl; ohne `fields` gilt die sparsame Voreinstellung. Der
  Kern holt die Karte über `share::card_for` mit den Rechten des Aufrufers. Ein Inhalt außerhalb
  seiner Reichweite macht den ganzen Befehl `not-found`. Geprüft wird vor der Freigabe und noch einmal
  bei „Allow“, mit derselben Reichweite samt Grenzen eines geliehenen Schlüssels (Vorbild
  `mail.respond_to_invitation`). Der Assistent ordnet den Befehl mit `items` auch dem Bereich der
  Inhalte zu (`contacts`).
- **Ablauf:**
  1. Anhangsordner aus `spreed.config.attachments.folder` (fehlt er: `/Talk`). Ist
     `attachments.allowed` falsch: `unsupported`.
  2. Die Datei der Karte per WebDAV nach `/remote.php/dav/files/<user-id>/<ordner>/<name>`, mit
     `If-None-Match: *`. Bei 412 hängt der Kern „ (2)“, „ (3)“ … an. Die Benutzer-ID kommt aus
     `GET /ocs/v2.php/cloud/user`, nie aus dem Loginnamen.
  3. Freigabe Art 10 in die Unterhaltung, `talkMetaData` mit `caption` = Text des Menschen oder
     Agenten, sonst der Text der Karte, und `referenceId`, damit eine Wiederholung nicht doppelt
     postet.
  4. Schlägt das Teilen fehl, löscht der Kern die eben hochgeladene Datei wieder (`If-Match`).
  - Ohne `media-caption` kommen zwei Nachrichten: erst der Text, dann die Datei.
  - Ohne Datei in der Karte geht nur der Text.
- **Größe:** höchstens 5 MB je Karte (die Grenze von `share_card`).
- **Freigabe:** Unterhaltung, Text, jede Karte („Contact card: Anna Muster (6 fields)“, „+ private
  address“) und „Uploads contacts.vcf to your Talk folder in Nextcloud“.
- **Rechte:** „Send on my behalf“ (Entscheidung 4) plus Lesen der Inhalte. Das Schreiben in die
  Dateien des Menschen braucht kein eigenes Recht: Es ist Teil des Teilens und steht in der Freigabe.
- **Teilen-Blatt:** Ziel „Chat“, sobald ein Konto mit Talk da ist und der Aufrufer `chat` hat. Es
  zeigt die letzten Unterhaltungen (`chat.list_conversations`) mit Suche und ein Feld für einen
  eigenen Text.

## Offen (in der jeweiligen Stufe klären)

- **Gruppenchats in der Kontaktliste:** Fassung A, B oder C (Entscheidung 17), zu wählen beim Durchsehen der
  Entwürfe, spätestens vor Stufe 3.
- **Klingeln als Benachrichtigung oder Fenster:** zu wählen beim Durchsehen, spätestens vor Stufe 5.
- **Antwort direkt in der Systembenachrichtigung:** macOS kann ein Textfeld in der Mitteilung
  (`UNTextInputNotificationAction`), das braucht AppKit-Code in `torro-appkit`. KDE kennt eine
  Erweiterung für Antworten in der Benachrichtigung, GNOME nicht. Eigener kleiner Plan nach Rückfrage. Dasselbe
  gilt für „Accept“ und „Decline“ in der Benachrichtigung eines Anrufs.
- **Lesestand über den Eingang:** `inbox.set_item_done` setzt an Talk-Einträgen schon heute den Lesestand in Talk,
  ohne Recht in `chat`. Vorschlag für Stufe 1: An Talk-Einträgen braucht es künftig auch Write in `chat` mit dem
  Konto in `chat_accounts`, wie Mail-Einträge Write in `mail` brauchen.
- **`inbox.reply`** aus [eingang.md](eingang.md) war als ein Antworten für Mail, Slack und Talk
  geplant. Für Talk antwortet das Paneel jetzt über `chat.send_message`. Stufe 1 passt die Zeile in
  [eingang.md](eingang.md) an; ob Mail und Slack ein gemeinsames `inbox.reply` brauchen, entscheiden
  deren Pläne.
- **Liste sparsamer holen:** Ob `GET /room?modifiedSince=…` auf allen unterstützten Versionen nur
  geänderte Unterhaltungen liefert, prüft Stufe 1.
- **Teams als Gruppen in der Kontaktliste** (App `circles`) und eine Obergrenze für große Instanzen: Stufe 2.
- **„Do not disturb“ an einen Fokus-Modus oder eine Workspace-Vorlage koppeln:** später.
- **Gruppen anlegen, Teilnehmer, Unterhaltungen verlassen oder archivieren:** nicht in diesem Konzept.
- **Gruppenanrufe, Bildschirm teilen, Hintergrund weichzeichnen:** später, nach Stufe 5.
- **Bilder im Verlauf** (Vorschau über `/core/preview`) und **Dateien hochladen** außer über Teilen:
  nicht in diesem Konzept, „nur Text“.
- **Tipp-Anzeige:** geht auch mit internem Signaling, kostet aber eine Sitzung und ein Long Polling je offenem
  Chatfenster. Kommt nach Stufe 5, wenn das Signaling für Anrufe ohnehin steht; mit HPB billiger.
- **TURN-Zugangsdaten im Anruffenster** (Datenschutz, Anrufe): Zustimmung des Menschen zur Ausnahme, erste Aufgabe
  der Stufe 5.
- **Ist TURN auf dem eigenen Server eingerichtet?** Zeigt nur eine angemeldete Abfrage; klärt der Versuch.
- **Matrix oder Slack als weitere Anbieter:** Die Bereiche `chat` und `presence` und das Modell in `torro-chat`
  sind dafür offen. Ein eigener Plan, wenn es so weit ist.
