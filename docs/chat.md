# Chat: Kontaktliste, Chatfenster und Anrufe auf Nextcloud Talk

Stand: 2026-11-11. **Konzept (#470), löst den Text-Chat-Plan aus #201 ab; Dateien und Medien mit #636; Umsetzung
mit #643 (Stand am Ende unter [Stand der Umsetzung](#stand-der-umsetzung)).** Machbarkeit:
[chat-machbarkeit.md](chat-machbarkeit.md). Entwürfe: [prototypes/chat/](prototypes/chat/index.html). Recherche zu
Status und Sichtbarkeit: [praesenz.md](praesenz.md). Aufgaben gibt es noch keine; die Umsetzung kommt als eigene
Issues je Stufe. Beantwortet die Ideen „Chat (Nextcloud Talk)“ und „Präsenz“ aus dem
[App-Katalog](app-catalog.md): **Wer ist gerade da, wer hat mir geschrieben, und wie erreiche ich ihn sofort?**

## Ziel

Ein leichter Chat der Suite auf **Nextcloud Talk**, schlicht wie früher ICQ, kein zweites Slack:

1. **Eine Kontaktliste** in einem eigenen, schmalen Fenster: alle Menschen der eigenen Nextcloud mit ihrem
   Status (online, abwesend, beschäftigt, nicht stören, offline) und ihrer Statusnachricht. Wer online ist, steht oben. Unten
   der eigene Status.
2. **Ein Chatfenster je Unterhaltung.** Ein Doppelklick auf einen Namen öffnet es; ist es schon offen, kommt es
   nach vorn. Darin Verlauf, Antwortfeld und die Knöpfe für Anrufe.
3. **Anrufe zu zweit**, mit Bild oder nur Ton, mit eigener Wahl von Kamera, Mikrofon und Lautsprecher. Wie sie
   technisch laufen, steht unter [Anrufe](#anrufe-stufe-6); die Entscheidung dazu unter
   [Entscheidungen](#entscheidungen), Punkt 13.
4. **Schnell antworten, wo die Nachricht ankommt.** Ungelesene Unterhaltungen stehen schon heute im Notification
   Hub (Eingangsquelle `talk:<login>@<host>`, Aufgabe E5.9). Dort kommen Verlauf und Antwortfeld dazu.

Dazu kommen **Dateien und Medien** (Stufe 4, #636): Dateien schicken, Bilder und Videos im Verlauf sehen,
Sprachnachrichten hören, Links als Karte, alles Geteilte einer Unterhaltung auf einen Blick. Darauf baut das **Teilen
in einen Chat** auf (Stufe 5, [teilen.md](teilen.md), TL2): Kontakte und später Notizen, Termine und Fotos als Karte
mit Datei in eine Unterhaltung geben.

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
| Datei teilen | Datei per WebDAV in den Anhangsordner, dann `POST /ocs/v2.php/apps/files_sharing/api/v1/shares` mit `shareType=10`, `shareWith=<token>`, `path`, `talkMetaData={"caption": …, "silent": …, "replyTo": …}` | `media-caption` |
| Sprachnachricht teilen | wie Datei teilen, mit `talkMetaData.messageType: "voice-message"` (nur abspielen, Entscheidung 19) | `voice-message-sharing` |
| Anhangsordner | `spreed.config.attachments.folder` (je Nutzer, meist `/Talk`), `attachments.allowed`; mit `attachments.conversation-subfolders` in `<ordner>/<Name>-<token>/<Anzeigename>-<user-id>/` | – |
| Große Dateien | WebDAV-Chunking v2 unter `/remote.php/dav/uploads/<user-id>/<upload-id>`: `MKCOL`, `PUT` der Stücke `1` … `10000` (5 MB bis 5 GB, das letzte kleiner), `MOVE` von `.file`; Kopf `Destination` bei jedem Schritt | Nextcloud `files.chunked_upload.max_size`, `max_parallel_count` |
| Datei in einer Nachricht | Objekt `file` in `messageParameters`: `id`, `name`, `size`, `mimetype`, `preview-available`, `hide-download`; nur bei Bildern `width`, `height`, `blurhash`; **keine Dauer** | – |
| Vorschaubild | `GET /index.php/core/preview?fileId=…&x=…&y=…&a=1` (PNG, JPEG, GIF, WebP; Videos nur, wenn der Admin `OC\Preview\Movie` mit `ffmpeg` einschaltet) | – |
| Geteilte Inhalte | `GET /api/v1/chat/{token}/share?objectType=…` (weiter über `X-Chat-Last-Given`) und `…/share/overview`; Arten `media`, `file`, `voice`, `audio`, `deckcard`, `location`, `other`, `recording` | `rich-object-list-media` |
| Link-Karten | Referenz-API von Nextcloud: `GET /ocs/v2.php/references/resolve?reference=<url>`, Bild unter `/index.php/core/references/preview/<id>` auf dem eigenen Server | Nextcloud ab 25 (alle unterstützten) |
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
Fenster und die Präsenz; 10, 13 bis 18 sind mit #470 (2026-11-06) dazugekommen, die Stufen (18) sind neu geschnitten.
19 bis 23 kommen aus #636 (2026-11-11, Dateien und Medien); mit ihnen bekam Entscheidung 18 die neue Stufe 4, und die Stufen
dahinter rückten um eins.

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
   Kontos, und umgekehrt. `presence.list_people` gibt nur Kennung, Anzeigename, Status, Statusnachricht und Ablauf heraus, nichts weiter
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
     `chat-sent` mit nur der Kennung der gesendeten Nachricht, ab Stufe 4 `chat-sending` mit nur der Kennung des
     Sendens und die Antworten von `prepare_file`, `upload_file`, `release_files` und `get_upload`, die nur
     Kennungen und die eigenen Angaben des Aufrufers tragen, sonst schickte ein Assistent mit einem Modell anderswo nie
     eine Datei; Fehler); jeder spätere Befehl ist damit von selbst
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
     der Liste. Ein Klingeln endet nach 45 s; die 60-s-Schleife reicht dafür nicht. Darum fragt der Kern die
     Benachrichtigungen jedes Kontos mit Talk alle 10 s ab, auch wenn kein Fenster des Chats offen ist, solange
     „Ring for incoming calls“ an ist (ab Stufe 6 ab Werk an; Schalter in den Einstellungen des Chats und in der
     Steuerzentrale unter General, Einstellung `chat_ring`); läuft `notify_push`, nutzt er das statt
     der Abfrage. Erst so kann er `torrochat` beim Klingeln starten. Wie schnell und wie teuer das wirklich ist,
     misst der Versuch der Stufe 6.
   - **Status** holt der Kern im Takt der Nextcloud-Schleife (60 s), solange die Kontaktliste offen ist oder ein
     Programm in den letzten 5 Minuten `presence.list_people` gefragt hat, sonst gar nicht
     ([praesenz.md](praesenz.md), „Idee für den Aufbau“, Punkt 2). So beantwortet Stufe 2 „Ist Ben gerade da?“ auch
     ohne Fenster: Die erste Frage holt frisch, die folgenden lesen den Speicher.
10. **Präsenz ist ein eigener Bereich `presence`**, kein Teil von `chat`: Den Status gibt es in Nextcloud auch
    ohne Talk (App User status), und wer einem Agenten Chats gibt, gibt ihm nicht von selbst, wer wann am Rechner
    sitzt.
    - **Wer in der Liste steht:** die Menschen aus dem Systemadressbuch des Kontos. Ist es gesperrt, die
      Menschen, mit denen es schon eine 1:1-Unterhaltung gibt, und wen der Mensch über die Suche
      (`presence.find_people`, die Sharee-Suche von Nextcloud, geht ohne Talk) hinzufügt (`presence.add_person`,
      gespeichert im Kern). Bleibt die Liste leer, sagt sie warum („Your Nextcloud does not show other
      users“).
    - **Abfragen:** Liefert `/statuses` etwas, reicht ein Aufruf (seitenweise). Ist die Liste leer, weil der Admin
      beschränkt hat, fragt der Kern einzeln (`/statuses/{userId}`), höchstens 50 Menschen je Minute; die übrigen
      stehen in der Gruppe „Offline“ mit „Status unknown“ als Unterzeile statt „Last seen …“ und zählen in Mini und
      Leiste nicht als online.
    - **Heartbeat ab Werk aus.** Der Schalter „Show others when I am online“ (Steuerzentrale und Einstellungen
      des Chats) schaltet ihn ein. Dann schickt der Dienst alle 5 Minuten `online` oder `away` nach der
      Leerlaufzeit des Systems, bei gesperrtem Bildschirm und im Ruhezustand nichts. Aus, weil „ich bin online“
      etwas über den Menschen verrät. Ohne Heartbeat zeigt die Liste unten „You appear offline“ mit dem
      Schalter daneben.
    - **Der eigene Status** (online, abwesend, beschäftigt, nicht stören, unsichtbar, Nachricht mit Ablauf) steht
      unten in der Kontaktliste und im Tray-Menü. Setzen ist nach außen (Entscheidung 4). „Online“ hält Nextcloud
      nur mit Heartbeat (ohne fällt es nach 15 Minuten auf offline): In Kontaktliste und Tray schaltet „Online“
      den Heartbeat bei Bedarf selbst ein (#653: die Wahl ist dieselbe Zustimmung wie der Schalter). Über die API
      und MCP bleibt es dabei: `presence.set_own_status` mit `online` ohne Heartbeat antwortet `unsupported`
      („Turn on “Show others when I am online” first.“). „Do not disturb“ stellt auch das
      Klingeln in der Suite ab.
    - **Mehrere Konten:** je Nextcloud-Konto eine Gruppe in der Liste, ab zwei Konten mit Kontonamen.
11. **Schnell antworten im Notification Hub** (Stufe 1), nicht im Tray. Das Tray zählt weiter nur
    bei „Inbox (3)“ und listet keine einzelnen Meldungen (Regel aus E3.3b). Die Benachrichtigung eines
    Talk-Eintrags bekommt den Knopf „Reply“. Er öffnet den Notification Hub beim Eintrag mit dem
    Cursor im Antwortfeld; ab Stufe 3 öffnet „Open chat“ das Chatfenster. Eine Antwort direkt in der
    Systembenachrichtigung braucht nativen Code je System und ist offen (siehe unten).
12. **Teilen in einen Chat (TL2)** gehört zum Chat, Stufe 5. `chat.send_message` bekommt
    `items` und `fields` wie `mail.save_draft` in TL1. Der Kern baut die Karte über `share::card_for` (aus TL1, noch nicht gebaut; Stufe 5 setzt TL1 voraus)
    mit den Rechten des Aufrufers, lädt die Datei in den Anhangsordner und teilt sie mit dem Text der
    Karte als Beschriftung. Mehr unter [Teilen in einen Chat](#teilen-in-einen-chat-stufe-5).
13. **Anrufe werden selbst gebaut** (entschieden vom Menschen am 2026-11-06, nach
    [chat-machbarkeit.md](chat-machbarkeit.md); empfohlen war, sie zuerst im Browser zu öffnen). Ein eigenes
    Anruffenster im Torro-Design auf Linux, macOS und Windows, mit Googles libwebrtc über die Crates `libwebrtc` und
    `webrtc-sys` von LiveKit (ohne LiveKit-Server), eigenem Talk-Signaling und dem Bild in Iced. Zuerst nur Anrufe zu
    zweit auf Servern mit internem Signaling (wie dem eigenen); HPB und Gruppenanrufe später. Stufe 6 beginnt mit
    einem Versuch als eigenem Issue (Abschnitt [Anrufe](#anrufe-stufe-6)). Bis das Anruffenster auf einem System
    läuft, öffnet „Accept“ den Anruf im Browser (`<server>/call/<token>`); „Open in Talk“ bleibt auch danach im Menü.
14. **Agenten rufen nicht an.** Anruf starten und annehmen, das Signaling eines Anrufs und die Wahl von Kamera und
    Mikrofon gibt es nur mit dem App-Schlüssel, also nur durch einen Menschen in der Oberfläche; Kamera und Mikrofon
    im Anruf schaltet das Anruffenster selbst, ohne API, wie beim Mikrofon des Assistenten
    ([api.md](api.md), „Was nie nach außen geht“, Punkt 6). Sonst könnte ein Agent im Namen des Menschen
    jemanden anrufen oder unbemerkt mithören und mitsehen. Agenten sehen höchstens, ob in einer Unterhaltung ein
    Anruf läuft (`chat.get_call`, `Read`). Bild und Ton gehen auf keinem Weg über API oder MCP. Das ist ein
    **vorgeschlagener neuer Punkt 11** für „Was nie nach außen geht“ (Wortlaut unter [Befehle](#befehle)); `api.md`
    nimmt den Teil zum Heartbeat mit der ersten Aufgabe der Stufe 2 auf, den Rest mit der ersten Aufgabe der Stufe 6. Ablehnen und Auflegen dürfen Agenten, weil beides nichts
    einschaltet.
15. **Fenster wie ICQ.** Kontaktliste und Chat sind zwei getrennte Fenster, keine Ansicht der Kontakte-App und
    kein Fenster mit Liste und Verlauf nebeneinander (so noch in #201). Je Unterhaltung höchstens ein
    Chatfenster; ein zweiter Doppelklick holt das offene nach vorn.
16. **Chat- und Anruffenster schweben.** Fenster-Manager mit Kacheln ordnen sonst jedes neue Fenster ein, und
    jeder Doppelklick auf einen Namen teilte den Bildschirm neu auf. Die Kontaktliste selbst ist eine normale
    Kachel (sie läuft dauerhaft, oft in einer Workspace-Vorlage), Chat-, Klingel-, Anruf- und Gerätefenster
    schweben. Regeln je Fenster-Manager unter [Fenster](#fenster).
17. **Gruppenchats über einen Umschalter „People | Groups“ oben in der Liste** (Fassung B, gewählt vom Menschen am
    2026-11-11 beim Durchsehen der Entwürfe, Entscheidung 24). Verworfen: (A) ein Abschnitt „Groups“ unter den
    Personen, (C) ein Knopf „Group chats…“ unten. Ein Gruppenchat öffnet sich im selben Chatfenster wie einer zu
    zweit. Gruppenanrufe kommen später.
18. **Stufen** (neu geschnitten mit #470, ersetzen CH1 bis CH4 aus #201):
    - **Stufe 1 · Text im Kern:** Bereich `chat`, API, MCP und schnell antworten im Notification Hub.
    - **Stufe 2 · Präsenz:** Bereich `presence`, Status anderer, eigener Status, Heartbeat, Tray-Menü.
    - **Stufe 3 · Fenster:** die App `torrochat` mit Kontaktliste und Chatfenstern, Schweben je Fenster-Manager,
      neue Unterhaltung zu zweit.
    - **Stufe 4 · Dateien und Medien** (#636): Dateien senden (Büroklammer, Ziehen, Einfügen), Bilder und Videos im
      Verlauf, Sprachnachrichten abspielen, Link-Karten, Medien-Übersicht, Öffnen und Sichern von Anhängen.
      Abschnitt [Dateien und Medien](#dateien-und-medien-stufe-4).
    - **Stufe 5 · Teilen in einen Chat** (TL2 aus [teilen.md](teilen.md)); nimmt den Ablauf zum Senden aus Stufe 4.
    - **Stufe 6 · Anrufe zu zweit**, selbst gebaut: (a) Versuch, (b) Klingeln, Annehmen und Ablehnen in der Suite
      (Anruf zunächst im Browser), (c) Anruffenster und Gerätewahl.
    - **Stufe 7 · Pflege:** eigene Nachrichten bearbeiten und löschen, Reaktionen, Favoriten,
      Benachrichtigungsstufe, Suche in Nachrichten.

    Jede Stufe ist für sich nutzbar und über alle drei Wege erreichbar, soweit Entscheidung 14 nichts anderes
    sagt. Dateien und Medien stehen als eigene Stufe direkt nach dem Fenster, weil man sie dort zuerst vermisst und
    das Teilen (Stufe 5) ihren Ablauf zum Senden nutzt; eine Stufe 7 am Ende ohne neue Nummern ist verworfen, weil die
    Nummern sonst nichts mehr über die Reihenfolge sagten.
19. **Sprachnachrichten: abspielen ja, aufnehmen später** (in der Planung von #636 entschieden). Der Verlauf spielt sie
    mit Wellenform ab. Eine Sprachnachricht ist Inhalt wie Text: Ihre Datei bekommt, wer `chat` lesen darf, über
    `chat.get_attachment`; das Chatfenster holt sie auf demselben Weg, spielt sie ab und rechnet die Wellenform aus
    den Samples. Nur das Abspielen ist Oberfläche ohne Befehl. Aufnehmen
    schaltet das Mikrofon ein und braucht dafür dieselbe Regel wie das Mikrofon des Assistenten
    ([api.md](api.md), „Was nie nach außen geht“, Punkt 6); das kommt mit einem eigenen Plan (siehe Offen).
20. **Agenten schicken nur Dateien, die sie selbst erzeugt oder hochgeladen haben** (in der Planung von #636
    entschieden). Ein Client nimmt als Quelle nur eine eigene Datei aus dem Register des Kerns (etwa eine PDF aus
    `tools.create_pdf`) oder lädt die Bytes selbst über `chat.upload_file` hoch, höchstens 25 MiB je Datei. Einen
    freien Pfad auf dem Rechner (`source: path`) nennt nur der App-Schlüssel, also ein Mensch in der Oberfläche:
    Sonst hieße „Send on my behalf“, dass ein Agent jede Datei, die er lesen kann, nach außen gibt. Senden mit Datei
    wartet ab Werk auf die Freigabe (Entscheidung 4).
21. **Vorschauen nur im Speicher**, wie der Verlauf (Entscheidung 7): höchstens 64 MiB je Konto, die ältesten gehen
    zuerst. Ein Zwischenspeicher auf der Platte wäre schneller beim Öffnen, müsste aber bei Remote Wipe sicher
    geleert werden und legte Bilder privater Chats auf den Rechner. Was ein Mensch öffnet oder sichert, liegt dagegen
    als Datei vor (eigener Anhangsordner des Chats, nach einem Tag gelöscht; Downloads).
22. **Videos öffnet das Programm des Systems** (entschieden vom Menschen am 2026-11-11, nach
    [chat-machbarkeit.md](chat-machbarkeit.md#dateien-und-medien-636), wie empfohlen). Ein Klick auf das Video lädt die
    Datei über den Kern (`chat.get_attachment`, für den App-Schlüssel bis 512 MiB) und öffnet sie dort; „Open in
    Nextcloud“ bleibt im Menü und ist der Weg für noch größere Videos. Kein
    GStreamer und kein FFmpeg in den Paketen; ein Player im Fenster wird ein eigenes Issue, wenn Videos im Alltag
    häufig sind.
23. **Link-Karten fremder Seiten nach Settings › Privacy** (entschieden vom Menschen am 2026-11-11, im Review von #636;
    seit #672 Teil der zentralen Freigabe fremder Inhalte, `docs/settings.md`, Abschnitt „Privacy“). Talk-Web löst den
    ersten Link beim Anzeigen auf; dabei ruft der eigene Server die fremde URL ab, und wer eine eindeutige URL schickt,
    erfährt, wann gelesen wurde. Darum bekommen nur Links auf den eigenen Nextcloud-Server ihre Karte immer von selbst.
    Für jeden anderen Link entscheidet die Stufe in Settings › Privacy: bei „Always show“ und für einen erlaubten
    Anbieter kommt die Karte von selbst, bei „Never show“ und für einen gesperrten nie (nur der Link im Text). Bei
    „Ask“ (Standard) fragt das Chatfenster für einen Anbieter, über den noch niemand entschieden hat, an der Stelle der
    Karte: „Show once“ (`chat.show_card`, nur Menschen, speichert nichts), „Always allow <Anbieter>“ und „Never for
    <Anbieter>“ (beide `settings.set_setting privacy_provider`, nur Menschen).
24. **Fassungen der Entwürfe gewählt** (vom Menschen am 2026-11-11, beim Durchsehen der Entwürfe zu #636):
    - **Gruppenchats:** Umschalter „People | Groups“ oben in der Kontaktliste (Entscheidung 17).
    - **Klingeln:** kleines Fenster mittig (`torro-chat-ring`), keine Benachrichtigung.
    - **Anruffenster:** zu zweit Bild in Bild (A); mit mehr als zwei Personen nebeneinander (B), sobald
      Gruppenanrufe kommen.
    - **Bild groß:** eigenes, schwebendes Fenster (`torro-chat-viewer`, B).
    - **Hochladen:** Leiste über dem Eingabefeld.
    - **Medien-Übersicht:** Seitenleiste im Chatfenster.

    Die verworfenen Fassungen bleiben in den Entwürfen zum Vergleich stehen.

## Aufbau

```
torroinbox (Paneel, Stufe 1) ───────┐
torrochat  (Kontaktliste, Chat-   ──┼─ IPC ─▶ torroworkplace (Kern)
            fenster, Anruffenster)  │          ├─ chat/service.rs   Befehle, Rechte-Filter, Freigabe-Fakten, Drossel
torrocontacts (Teilen) ─────────────┘          ├─ chat/cache.rs     Unterhaltungen und Nachrichten im Speicher
   │                                           ├─ chat/live.rs      Long Polling der vorgemerkten Unterhaltungen, Klingeln
   │ Medien (Stufe 6):                         ├─ chat/call.rs      Sitzung, Anruf beitreten/verlassen, Signaling (Stufe 6)
   │ libwebrtc, Kamera, Ton,                   ├─ chat/media.rs     Dateien vorbereiten, senden, Vorschauen, Karten (Stufe 4)
   │                                           ├─ chat/share.rs     Karte → Datei → Ablauf aus media.rs (Stufe 5)
   │ Bild in Iced                              ├─ presence/         Status anderer, eigener Status, Heartbeat (Stufe 2)
   │ (nur im Anruffenster)                     └─ sources/nextcloud.rs, sources/talk.rs  Liste alle 60 s (gibt es)
   ▼                                                    │
 Gegenstelle (Browser, Talk-App)        torro_chat::ChatProvider (Trait, Modell)
   ◀──── WebRTC (Bild, Ton), STUN/TURN ────▶ └─ torro-nextcloud: talk.rs, user_status.rs,
                                                  files.rs (WebDAV, Chunking, Vorschau, Freigabe Art 10)
```

Der Kern spricht mit Talk (Anmeldung, Sitzung, Signaling), die App nur mit dem Kern. Bild und Ton laufen dagegen
direkt zwischen dem Anruffenster und der Gegenstelle: Kamera und Mikrofon gehören dem Fenster, in dem man sie
sieht, und macOS fragt die Erlaubnis für genau dieses Programm ab. Das ist eine Ausnahme von „Oberflächen lesen und
schreiben nichts selbst“ (CLAUDE.md, [api.md](api.md)); sie gilt erst, wenn der Mensch ihr zustimmt (Offen, Stufe 6),
und steht dann in Punkt 11.

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
  - bei Dateien (Stufe 4) je Datei Name, Größe, Art, Herkunft („made by this agent“, „uploaded by this agent“) und
    eine Vorschau, dazu „Uploads <name> to your Talk folder in Nextcloud“ (Abschnitt
    [Dateien und Medien](#dateien-und-medien-stufe-4));
  - bei Karten (Stufe 5) jede Karte wie bei Mail und dass eine Datei in den Talk-Ordner geladen wird.

  Namen gehen durch dieselbe Entschärfung wie in anderen Freigaben (eine Zeile, 60 Zeichen). Kennt
  der Cache eine Unterhaltung nicht mehr, steht dort „unknown conversation“.
- **Wartende Freigaben auf der Platte:** Bis der Mensch entscheidet, liegt der Request mit dem Text in
  der Datei der Freigaben (0600). Das ist die einzige Stelle, an der Chat-Text auf die Platte kommt
  (Entscheidung 7). Heute hält der Kern entschiedene und abgelaufene Freigaben noch einen Tag samt
  Request; bei `chat.*` und `presence.set_own_status` leert er den Text darum sofort bei der Entscheidung oder
  dem Ablauf, ebenso die Dateinamen und Bytes einer Datei (Stufe 4).
- **Drossel:** Ein Client (gezählt je Client-Kennung, damit geliehene Schlüssel des Assistenten sie
  nicht umgehen) führt höchstens 30 Nach-außen-Befehle in `chat` und `presence` je Stunde und 5 je Minute aus
  (Senden, Bearbeiten, Löschen, Reaktionen, neue Unterhaltung, eigener Status zusammen), sonst `busy`. Auch mit
  „Free“ kann ein entgleister Agent so niemanden zuschütten. Der App-Schlüssel ist nicht gedrosselt.
- **Erwähnungen von allen** (`@all`) sind für Clients nicht verboten, stehen aber in der Freigabe.
- **Links und Dateien:** Dateien in Nachrichten zeigt das Chatfenster mit Name, Größe und „Open in
  Nextcloud“. Der Link wird aus der Kennung der Datei auf dem Ursprung des Kontos gebaut
  (`<server>/f/<id>`), nie aus der Antwort übernommen (Regel 15 aus E5). Links im Text öffnen sich nur
  nach einem Klick und nur mit `https`, `mailto`.
- **Bilder** (ersetzt mit #636 die Regel „Bilder lädt der Verlauf nicht“):
  - **Nur vom Ursprung des Kontos.** Vorschauen holt der Kern über `/index.php/core/preview` mit der Kennung der
    Datei, Bilder von Link-Karten über `/index.php/core/references/preview/<id>`, beides auf dem Ursprung des Kontos
    gebaut. Nie über eine URL aus der Antwort von Talk oder aus `openGraphObject` (`thumb`, `link`): Ein Bild von
    einem fremden Server verriete, wann und wo der Mensch die Nachricht liest. Gibt es kein Bild vom eigenen Server,
    gibt es keins.
  - **Nur im Speicher** (Entscheidung 21): höchstens 64 MiB Vorschauen je Konto. Geöffnete Anhänge liegen in einem
    eigenen Ordner des Chats, `<cache>/chat-attachments/<konto>/` (Ordner 0700, Dateien 0600, nach einem Tag
    gelöscht), nicht im Anhangsordner der Mail: `mail.save_draft` nimmt jede Datei aus jenem Ordner an und prüft nur
    den Ort, ein Agent mit `mail`, aber ohne `chat`, hängte sonst Chat-Bilder an einen Entwurf. `mail.save_draft` nimmt
    aus dem Ordner des Chats nichts an.
  - **Kein SVG**, auch nicht als Anhang in der Vorschau (es kann Skripte und Verweise nach außen enthalten); Nextcloud
    liefert Vorschauen ohnehin nur als Rasterbild, und der Kern schickt `mimeFallback` nie, das auf ein SVG-Symbol
    umleiten würde. Bilder prüft der Kern wie `mail.upload_image` (PNG, JPEG, GIF, WebP, Größe und Kanten begrenzt).
  - **Keine Dateinamen und keine WebDAV-Pfade** in Protokoll und Fehlern; Dateinamen sind fremder Text
    (`untrusted`, Entscheidung 5). Keine Bytes in `Debug`, Protokoll oder der Hülle einer Freigabe (das Feld `base64`
    von `upload_file` bekommt wie bei der Mail einen Typ mit geschwärztem `Debug`).
  - **Föderierte Unterhaltungen:** Dort kommen die Parameter `file` vom fremden Server und könnten auf eine Datei im
    eigenen Speicher zeigen. Der Kern nimmt Dateien einer Nachricht nur an, wenn sie zu den Freigaben der Unterhaltung
    gehören (`…/share`). Weil auch diese Liste in föderierten Unterhaltungen vom fremden Server kommen kann (*vermutet*),
    gilt bis zu einer Prüfung in Stufe 4: Dort zeigt die Oberfläche Dateien nur als Zeile mit „Open in Nextcloud“,
    und `get_preview`, `get_attachment` und `save_attachment` antworten `unsupported`, für jeden Schlüssel.
  - **Fremde Dateien nie ungeprüft starten.** „Open“ gibt es nicht für ausführbare und aktive Arten (Programme,
    Skripte, `.app`, `.command`, `.desktop`, `.jar`, Installationspakete, HTML, SVG; erkannt an Endung **und** Bytes):
    Sie bekommen nur „Download“ und „Open in Nextcloud“. Auf macOS setzt der Kern an jeder geholten Datei das Attribut
    `com.apple.quarantine`, damit Gatekeeper fragt, wie bei einem Download im Browser.
- **Konto weg oder Remote Wipe:** Cache und Status des Kontos werden geleert, laufende Long-Polling-Anfragen
  und Anrufe werden beendet, das Konto fällt aus allen `chat_accounts` und `presence_accounts`. Ebenso werden
  Vorschauen, vorbereitete Dateien und laufende Uploads des Kontos verworfen (halbe Uploads auf dem Server gelöscht,
  soweit er erreichbar ist) und die geöffneten Anhänge des Kontos im Anhangsordner gelöscht.
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
    in der ersten Aufgabe der Stufe 6:** ein einmaliger Never-Befehl `chat.call_credentials { call }`, gültig nur für
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
    `chat_speaker` mit der Kennung des Geräts), nicht im Protokoll, nicht in Freigaben und nicht in der Antwort von
    `settings.get_settings` an Clients.

## Fenster

Entwürfe: [prototypes/chat/](prototypes/chat/index.html) (alle Fenster in hell und dunkel, Fassungen
umschaltbar; gewählt ist, was Entscheidung 24 nennt). Alle Fenster gehören zur App `torrochat` und folgen dem Torro-Design
([`torro-design`](../.claude/skills/torro-design/SKILL.md)): Inter, Tokens des aktiven Designs, Status nie nur über
Farbe, Rot nur für den einen Primärknopf.

**Kontaktliste** (Fensterklasse `torro-chat`, Kachel, schmal, hoch, beim Start 320 × 760):
- **Obere Zeile:** „Contacts“, rechts die Einstellungen (`shell::SETTINGS_ICON`).
- **Suchfeld** filtert Namen und Statusnachrichten in der Liste; Enter öffnet den ersten Treffer. Ohne Treffer
  „Find on server…“ (`presence.find_people`), das Ergebnis kommt mit „Add to list“ in die Liste.
- **Gruppen nach Status:** „Online“, „Away“, „Busy“, „Do not disturb“, „Offline“ in dieser Reihenfolge, jede mit Zahl und
  einklappbar; „Offline“ ab Werk zugeklappt, sobald mehr als 20 Menschen darin stehen. In jeder Gruppe nach Name.
  Jede Zeile: Avatar mit Status-Marke (jede Marke mit eigener Form: Punkt, Mond, halber Kreis, Minus, Ring), Name, darunter die
  Statusnachricht oder „Last seen yesterday“, rechts die Zahl ungelesener Nachrichten.
- **Gruppenchats** über den Umschalter „People | Groups“ oben in der Liste (Entscheidung 17).
- **Fuß:** eigener Status mit Avatar und Nachricht, ein Klick öffnet ein Menü mit den Status, „Set status
  message…“ (mit Ablauf: 30 min, 1 h, heute, diese Woche) und „Show others when I am online“.
- **Bedienung:** Pfeiltasten wandern, Enter oder Doppelklick öffnet das Chatfenster, `⌘F`/`Ctrl+F` springt in die
  Suche. Kontextmenü: „Open chat“, „Audio call“, „Video call“, „Show contact“ (sobald es die Kontakte-App `torrocontacts` gibt; öffnet sie beim Kontakt mit
  derselben Mail-Adresse bzw. Nextcloud-Kennung; die Zuordnung macht der Kern, ohne Treffer fehlt der Eintrag),
  „Remove from list“ (nur für selbst hinzugefügte).
- **Größen:** Ab 280 Breite und 280 Höhe (`CONTACTS_LIST_MIN_WIDTH`, #652) bleibt es die volle Liste, auch in „Mini
  hoch“, damit sie ohne Tiling-Manager (macOS) nutzbar ist. Darunter zeigt Mini die Zahl der Menschen online und die
  ungelesenen Unterhaltungen; Leiste die Avatare der Menschen online. In einer breiten Kachel wird die Liste höchstens `size::content_max()` breit, nicht gestreckt.
- **Leer:** ohne Konto „Connect Nextcloud“ (öffnet die Steuerzentrale), ohne User status „Your Nextcloud does not
  share status“, ohne sichtbare Menschen „Your Nextcloud does not show other users“ mit der Suche.

**Chatfenster** (Klasse `torro-chat-conversation`, schwebend, 440 × 680 gedacht; ob Größe und Ort je
Unterhaltung gemerkt werden können, siehe „Schweben“):
- **Kopf:** Avatar mit Status, Name, darunter Status und Statusnachricht (bei Gruppen die Zahl der Teilnehmer),
  rechts „Audio call“, „Video call“ und „…“ (Mark as unread, Notifications, Open in Talk, Show contact).
- **Verlauf:** Nachrichten nach Tag gruppiert, eigene rechts. Zitat bei Antworten, Erwähnungen hervorgehoben.
  Einfaches Markdown (fett, kursiv, Code, Links). Trenner „New“ beim Lesestand. Systemnachrichten klein in `TEXT_3`,
  verpasste Anrufe mit Icon. Nach oben scrollen lädt ältere. Dateien je nach Art (Stufe 4, ab Stufe 3 alle als
  Datei-Zeile):
  - **Bild:** Vorschau (höchstens 220 × 220, Seitenverhältnis aus `width`/`height`, bis sie da ist ein ruhiger
    Platzhalter in der Größe, damit nichts springt), darunter die Beschriftung. Ein Klick öffnet „Bild groß“.
  - **Video:** Vorschaubild mit Abspielknopf, wenn der Server eins hat; die Dauer nur, wenn der Kern sie kennt
    (Talk liefert keine). Ein Klick öffnet das Video im Programm des Systems (Entscheidung 22), der Tooltip sagt es.
  - **Sprachnachricht:** Abspielknopf, Wellenform, Dauer; spielt im Chatfenster (Entscheidung 19).
  - **Andere Datei** und jeder **Rückfall ohne Vorschau** (keine Vorschau, SVG, `hide-download`, Format ohne
    Decoder): Zeile mit Symbol der Art, Name, Größe und Art, Knöpfe „Open“, „Download“, „Open in Nextcloud“
    (`hide-download`: nur „Open in Nextcloud“).
  - **Link-Karte** unter einer Nachricht mit Link: Bild vom eigenen Server, Titel, Domain; nur der erste Link einer
    Nachricht, wie in Talk-Web. Links auf den eigenen Nextcloud-Server bekommen sie von selbst, andere nach Settings ›
    Privacy (Entscheidung 23, #672). Bei „Ask“ steht statt der Karte ein ruhiger Satz „This message shows content from
    <Anbieter>.“ mit den Knöpfen „Show once“, „Always allow <Anbieter>“ und „Never for <Anbieter>“; die Antwort gilt
    für alle Nachrichten des Anbieters im Fenster. Ohne Karte bleibt der Link im Text.
- **Eingabe:** mehrzeilig, Enter sendet, Shift+Enter bricht um. `@` öffnet Vorschläge. Ein Zähler erscheint ab
  90 % der Höchstlänge. Im Menü „Send without notification“. Ab Stufe 4 links die Büroklammer („Attach file“, Dialog
  des Systems), Ziehen einer Datei auf das Fenster (die ganze Fläche wird zur Ablage, „Drop to send to Anna Berg“)
  und Einfügen eines Bildes aus der Zwischenablage. Der Text im Feld wird die Beschriftung. Vor dem Senden stehen
  die Dateien als **Leiste über dem Feld** (Entscheidung 24); nach dem Senden zeigt dieselbe Stelle den Fortschritt mit ✕ („Cancel upload“). Zu groß, kein Platz auf
  dem Server oder `attachments.allowed` aus: Meldung an der Datei, der Text bleibt im Feld.
- **Bild groß** (Stufe 4): ein eigenes, schwebendes Fenster (Klasse `torro-chat-viewer`, Entscheidung 24). Bild eingepasst, oben Name, „Download“, „Open in Nextcloud“ und
  Schließen (`Esc`), `←`/`→` blättern durch die Bilder der Unterhaltung, unten „2 / 5 · Anna Berg · 10:02“. Das große
  Bild ist eine Vorschau in Fenstergröße, nicht das Original.
- **Medien-Übersicht** (Stufe 4, Knopf „Shared media“ im Kopf): Reiter „Media“ (Raster der Bilder und Videos nach
  Monat, neueste oben), „Files“, „Voice“ und „Links“ (die Link-Karten aus dem Cache); jede Zeile mit „Show in chat“.
  Seitenleiste im Chatfenster (Entscheidung 24). Ohne
  `rich-object-list-media` fehlt der Knopf.
- **Gelesen:** Ist das Fenster im Vordergrund und die neueste Nachricht sichtbar, setzt die App den Lesestand.
- **Tasten:** `Esc` bricht eine Antwort ab bzw. schließt das Fenster. Anrufe starten bewusst nur über die Knöpfe,
  ohne Taste: Eine Taste im Eingabefeld (etwa `Ctrl+Shift+V`, „ohne Formatierung einfügen“) schaltete sonst
  versehentlich Kamera und Mikrofon ein.
- **Leer und Fehler:** Offline zeigt den Speicher mit „Offline“ im Kopf; eine verschwundene Unterhaltung „This
  conversation is no longer available“.

**Eingehender Anruf:** ein **kleines Fenster** mittig (Klasse `torro-chat-ring`, schwebend, 340 × 360) mit Decline,
Audio only und Accept (Entscheidung 24; die Benachrichtigung oben rechts ist verworfen). „Accept“
ist der Primärknopf. Bei „Do not disturb“ klingelt nichts; der Anruf steht danach als verpasst im Chat und im
Notification Hub. Ein Klingeln endet nach 45 s oder wenn die Gegenseite auflegt.

**Anruffenster** (Klasse `torro-chat-call`, schwebend, frei in der Größe, 760 × 500 gedacht): zu zweit Bild
in Bild (die andere Person groß, das eigene Bild klein in einer Ecke, verschiebbar), mit mehr als zwei Personen
nebeneinander, sobald Gruppenanrufe kommen (Entscheidung 24). Unten
Mikrofon (`M`), Kamera (`V`), Geräte, Bildschirm teilen (ausgegraut, später) und „Leave call“ als Primärknopf.
Oben Dauer und „Show chat“. Schließen beendet den Anruf.

**Gerätewahl** (Klasse `torro-chat-devices`, schwebend, 420 × 660): Kamera mit Vorschau („Preview stays on this
computer“), Mikrofon mit Pegel, Lautsprecher mit „Play test sound“, Schalter „Join calls with camera off“, „Done“.
Ohne Gerät oder ohne Recht dazu steht der Weg zur Systemeinstellung da. Ein neu eingestecktes Gerät erscheint
sofort in der Liste, gewechselt wird nur hier.

**Schweben je Fenster-Manager** (Entscheidung 16): Der Kern setzt die Regeln für die festen Klassen des Chats
beim Start selbst über `torro-wm`, wie heute für `WORKPLACE_DIALOG_CLASS`; einen Request dafür gibt es nicht:
- **Hyprland:** `float_dialogs` gibt es schon (Regel `float = true, center = true` je Klasse). Ein Neuladen der
  Konfiguration verwirft Laufzeit-Regeln (`ConfigReloaded` in `torro-wm`); der Kern muss sie darauf neu setzen,
  das tut er heute für keine Klasse und kommt mit Stufe 3 dazu. Das reicht für Klingel-, Anruf- und Gerätefenster. Ein Chatfenster an einem
  gemerkten Ort bräuchte eine Regel ohne `center` und ein Verschieben durch den Kern (unter Wayland darf das
  Programm sein Fenster nicht selbst setzen, [modes.md](modes.md)); das ist eine neue Methode am Trait
  `WindowManager`, dessen Schnittstelle festgelegt ist, also eine Rückfrage in Stufe 3. Bis dahin öffnen
  Chatfenster mittig.
- **AeroSpace und GlazeWM:** `float_dialogs` tut dort heute nichts, und die „Klasse“ ist die Bundle-ID bzw. der
  Prozessname: Alle Fenster von `torrochat` teilen sie, auch die Kontaktliste. Unterscheiden ließen sie sich nur am
  Fenstertitel (AeroSpace `on-window-detected` mit `window-title-regex-substring`, GlazeWM `window_rules` mit
  `window_title`) oder indem Chat- und Anruffenster ein eigener Prozess bzw. ein eigenes Bündel werden. Welcher Weg,
  klärt Stufe 3 mit Rückfrage; bis dahin schweben sie dort nicht von selbst.
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
status bei den Diensten. Unter General die Schalter „Show others when I am online“ und „Ring for incoming calls“.

**Teilen-Blatt (Stufe 5):** Ziel „Chat“ mit den letzten Unterhaltungen und Suche, siehe unten.

**Demo:** `torrochat --demo` mit sechs Menschen in allen Status, zwei Gruppen, drei Unterhaltungen mit Erwähnung,
Datei, Systemnachricht und verpasstem Anruf; ab Stufe 4 dazu Bild, Video mit und ohne Vorschau, Sprachnachricht und
Link-Karte. `--open contacts | chat | ring | call | devices | media | viewer | upload` öffnet ein Fenster bzw. einen
Zustand direkt (für die Sichtprüfung).

## Befehle

Geprüft mit dem Agenten `torro-api-review` (2026-11-06); seine Funde sind eingearbeitet. Die Namen der Rust-Fälle
beginnen mit `Chat…` bzw. `Presence…` (`ChatSendMessage`), damit sie nicht mit den Requests des Assistenten
(`OpenChatWindow`, KI-Platz `chat`) verwechselt werden; die Rechte-Zeile heißt „Chat (Talk)“.

Bereich `chat`:

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| 1 | `chat.list_conversations { account?, unread_only?, query?, limit? }` (Name, Art, ungelesen, Erwähnung, letzte Nachricht, läuft ein Anruf; ab Stufe 6 auch: klingelt es) | Read | `chat_list_conversations` |
| 1 | `chat.get_messages { conversation, before?, after?, limit? }` (höchstens 100, ändert keinen Lesestand) | Read | `chat_get_messages` |
| 1 | `chat.send_message { conversation, text, reply_to?, silent? }` | Write + **nach außen** | `chat_send_message` |
| 1 | `chat.mark_read { conversation, up_to? }` | Write | `chat_mark_read` |
| 1 | `inbox.get_item` mit dem neuen Feld `conversation` bei Talk-Einträgen (Erweiterung) | Read (`inbox`) | `inbox_get_item` (vorhanden) |
| 1 | `inbox.open_item` öffnet bei Talk-Einträgen den Notification Hub beim Eintrag mit dem Antwortfeld (Erweiterung, wie heute bei Mail) | wie heute | `inbox_open_item` (vorhanden) |
| 2 | siehe Bereich `presence` | | |
| 3 | `chat.mark_unread { conversation }` | Write | `chat_mark_unread` |
| 3 | `chat.suggest_mentions { conversation, query, limit? }` | Read | `chat_suggest_mentions` |
| 3 | `chat.find_people { account, query, limit? }` (Personen des Servers mit Talk für eine neue Unterhaltung) | Read | `chat_find_people` |
| 3 | `chat.create_conversation { account, with }` (1:1, gibt eine vorhandene zurück) | Write + **nach außen** | `chat_create_conversation` |
| 3 | `apps.open_app` mit `target: "chat:<unterhaltung>"` bzw. `"chat:contacts"` (Chatfenster bzw. Kontaktliste; eine Unterhaltung außerhalb von `chat_accounts` ist `not-found`. Anders als andere Ziele, die immer ein neues Fenster starten, holt `chat:` ein offenes Chatfenster nach vorn (Entscheidung 15); wie der Kern das der laufenden App sagt, klärt Stufe 3 mit Rückfrage, weil `OpenApp` heute immer startet. Erweiterung) | Write (`apps`) | `apps_open_app` (vorhanden) |
| 3 | `apps.open_app` mit `target: "contact:<id>"` („Show contact“: `torrocontacts` beim Kontakt; braucht Read in `contacts` und das Adressbuch in `address_books`, sonst `not-found`, Erweiterung) | Write (`apps`) | `apps_open_app` (vorhanden) |
| 4 | `chat.prepare_file { account, source }` (`source`: `registered` mit der Kennung einer eigenen Datei aus dem Register oder `path`, **nur App-Schlüssel**; gibt Kennung, Name, Größe, Art, Maße und eine kleine Vorschau zurück; lagert nur im Kern) | Write | `chat_prepare_file` |
| 4 | `chat.upload_file { account, upload?, offset, base64, name?, final? }` (Base64 in Stücken von höchstens 256 KiB; `final` gibt wie `prepare_file` eine vorbereitete Datei zurück, danach unveränderlich) | Write | `chat_upload_file` |
| 4 | `chat.release_files { account, ids }` (eigene vorbereitete Dateien verwerfen; fremde und unbekannte werden übergangen) | Delete | `chat_release_files` |
| 4 | `chat.send_message` mit `files` (Kennungen aus `prepare_file`, Erweiterung) | Write + **nach außen** | `chat_send_message` (vorhanden) |
| 4 | `chat.get_upload { id }` (Fortschritt des Sendens: Bytes, Stand, Fehler) | Read | `chat_get_upload` |
| 4 | `chat.cancel_upload { id }` (Senden abbrechen, die halbe Datei auf dem Server löschen) | Write | `chat_cancel_upload` |
| 4 | `chat.get_preview { id, file?, card?, size }` (`id` die Nachricht, `file` eine Datei dieser Nachricht oder `card: true` das Bild ihrer Link-Karte, `size` `small` oder `large`; gibt ein Rasterbild zurück) | Read | `chat_get_preview` (liefert ein Bild) |
| 4 | `chat.get_attachment { id, file }` (Datei einer Nachricht in den Anhangsordner des Chats, Pfad, Größe, Art; wie `mail.get_attachment`) | Read | `chat_get_attachment` |
| 4 | `chat.save_attachment { id, file, to }` (`to`: `downloads`, `{ "target": … }`, `file` und `folder` **nur Menschen**) | Write | `chat_save_attachment` |
| 4 | `chat.list_shared { conversation, kind?, before?, limit? }` (`kind`: `media`, `file`, `voice`, `link`, …; höchstens 100) | Read | `chat_list_shared` |
| 4 | `chat.show_card { id }` (Link-Karte zum ersten Link der Nachricht holen, „Show once“; der Server ruft dafür die fremde URL ab; `unsupported` für einen gesperrten Anbieter und bei „Never show“, #672) | Write | `Never` (nur Menschen, siehe Vorschlag für `api.md`) |
| 4 | `chat.get_messages` mit dem neuen Feld `cards` (Link-Karten aus dem Cache) und der Eingabe `around` (Nachrichten um eine Kennung, für „Show in chat“; Erweiterung) | Read | `chat_get_messages` (vorhanden) |
| 5 | `chat.send_message` mit `items` und `fields` (Erweiterung, TL2) | Write + **nach außen** + Read der Inhalte | `chat_send_message` (vorhanden) |
| 6 | `chat.get_call { conversation }` (läuft ein Anruf, klingelt es, seit wann, wie viele sind drin, ob der Mensch dabei ist; keine Namen, keine Geräte, kein Bild) | Read | `chat_get_call` |
| 6 | `chat.start_call { conversation, video }` (Anruffenster öffnen, beitreten, Gegenseite klingeln lassen) | Write + nach außen | `Never` (Punkt 11) |
| 6 | `chat.answer_call { conversation, video }` (annehmen, mit Bild oder nur Ton) | Write + nach außen | `Never` (Punkt 11) |
| 6 | `chat.decline_call { conversation }` (eigenes Klingeln beenden, die Gegenseite telefoniert weiter) | Write | `chat_decline_call` |
| 6 | `chat.leave_call { conversation }` (auflegen, schließt das Anruffenster) | Write | `chat_leave_call` |
| 6 | `chat.call_events { call, after? }` (Signaling-Nachrichten und Teilnehmer für das Anruffenster) | Read | `Never` (Punkt 11) |
| 6 | `chat.call_signal { call, messages }` (Signaling-Nachrichten des Anruffensters an Talk) | Write | `Never` (Punkt 11) |
| 6 | `chat.call_credentials { call }` (STUN/TURN für den laufenden Anruf, einmal je Anruf; nur nach Zustimmung zur Ausnahme, siehe Anrufe) | Read | `Never` (Punkte 1 und 11) |
| 6 | `settings.set_setting` mit `chat_camera`, `chat_microphone`, `chat_speaker` (Kennung des Geräts) und `chat_join_camera_off` (Erweiterung) | Admin (`settings`); Geräte und `chat_join_camera_off: false` **nur Menschen** | `settings_set_setting` (vorhanden) |
| 6 | `settings.set_setting` mit `chat_ring` („Ring for incoming calls“: Abfrage alle 10 s und Start von `torrochat` beim Klingeln; Erweiterung) | Admin (`settings`) | `settings_set_setting` (vorhanden) |
| 7 | `chat.edit_message { id, text }` | **Delete** + **nach außen** | `chat_edit_message` |
| 7 | `chat.delete_message { id }` | Delete + **nach außen** | `chat_delete_message` |
| 7 | `chat.react { id, emoji, remove? }` | Write + **nach außen** | `chat_react` |
| 7 | `chat.set_favorite { conversation, favorite }` | Write | `chat_set_favorite` |
| 7 | `chat.set_notification_level { conversation, level }` (`default`, `always`, `mention`, `never`) | Write | `chat_set_notification_level` |
| 7 | `chat.search_messages { query, account?, limit? }` (Unified Search von Talk) | Read | `chat_search_messages` |

Bereich `presence` (Rechte: Stufe in `presence` und eigene Auswahl `presence_accounts`, Entscheidung 3):

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| 2 | `presence.list_people { account? }` (Kennung, Anzeigename, Status, Statusnachricht, Ablauf; nichts weiter aus dem Systemadressbuch; `untrusted: true`) | Read | `presence_list_people` |
| 2 | `presence.add_person { account, person }` / `presence.remove_person { account, person }` (eigene Liste, wenn das Systemadressbuch gesperrt ist; gespeichert im Kern, `presence.json` im Konfigurationsordner) | Write | `presence_add_person`, `presence_remove_person` |
| 2 | `presence.find_people { account, query, limit? }` (Sharee-Suche von Nextcloud, geht ohne Talk; höchstens 20 Treffer, mindestens zwei Zeichen, `untrusted: true`) | Read | `presence_find_people` |
| 2 | `presence.get_own_status { account }` | Read | `presence_get_own_status` |
| 2 | `presence.set_own_status { account, status, message?, icon?, clear_at? }` | Write + **nach außen** | `presence_set_own_status` |
| 2 | `settings.set_setting` mit `presence_heartbeat` (Heartbeat) | Admin (`settings`); `true` **nur Menschen** | `settings_set_setting` (vorhanden) |

Es gibt keinen Befehl für das Schweben der Fenster: Der Kern setzt die Regeln für die festen Fensterklassen des
Chats beim Start selbst, wie heute für `WORKPLACE_DIALOG_CLASS`. So kann auch kein Client fremde Klassen schweben
lassen. Eingeklappte Gruppen merkt die Kontaktliste in `ui.json` (`torro_gui::ui_state`); Größe und Ort heute
nicht (siehe Fenster, „Schweben“).

**Vorgeschlagener Punkt 11 für [„Was nie nach außen geht“](api.md#was-nie-nach-außen-geht)** (`api.md` nimmt
den Teil zum Heartbeat mit der ersten Aufgabe der Stufe 2 auf, den Rest mit der ersten Aufgabe der Stufe 6, samt Tests wie
`the_microphone_is_switched_on_by_people_only` und den Never-Befehlen in `kept_back` von `names_follow_the_rules`):

> 11. **Anrufe und Kamera:** nur mit dem App-Schlüssel, also nur durch einen Menschen in der Oberfläche
>     (Chatfenster, Klingeln, Anruffenster, Gerätewahl, Einstellungen des Chats, Steuerzentrale).
>     - **Anruf starten und annehmen** (`chat.start_call`, `chat.answer_call`): Sonst könnte ein Agent im Namen
>       des Menschen jemanden anrufen oder Mikrofon und Kamera in einen Anruf schalten.
>     - **Das Signaling eines Anrufs** (`chat.call_events`, `chat.call_signal`, `chat.call_credentials`): Wer es
>       hat, kann einem laufenden Anruf beitreten und mithören.
>     - **Geräte wählen** (`settings.set_setting` mit `chat_camera`, `chat_microphone`, `chat_speaker`) und
>       **mit Kamera beitreten** (`chat_join_camera_off: false`): Die Wahl bereitet vor, welche Kamera und welches
>       Mikrofon beim nächsten Anruf angehen. Wie beim Mikrofon entscheidet der Wert (`Request::kept_for_people`);
>       `chat_join_camera_off: true` darf jeder mit Admin in `settings`. Die Kennungen der Geräte gibt
>       `settings.get_settings` nur dem App-Schlüssel heraus (Clients sehen „set“), weil Gerätenamen oft Name und
>       Rechner des Menschen verraten.
>     - **Medien gehen nie über den Kern:** Bild und Ton eines Anrufs, die Vorschau der Kamera, der Pegel des
>       Mikrofons und der Testton laufen nur im Anruffenster bzw. in der Gerätewahl, die dafür selbst auf die
>       Geräte zugreifen. Das ist neben dem Ansichtszustand in `ui.json` eine Ausnahme von „Oberflächen lesen und schreiben nichts
>       selbst“: macOS
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
- **„Nach außen“** sind `send_message`, `create_conversation`, `edit_message`, `delete_message`, `react`,
  `set_own_status` und, obwohl Clients sie wegen `Never` ohnehin nicht bekommen, `start_call` und `answer_call` (die
  Gegenseite klingelt bzw. sieht einen beitreten): Sie kommen in `OUTBOUND` (`torro-ipc`, `api.rs`). Mit dem Text
  von `Lack::Outbound` werden auch der Doc-Kommentar an `Lack::area()` („always mail“) und der an `Rights::outbound`
  („send mail …“) neutral (Entscheidung 4).
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
- **Dateien und Medien (Stufe 4)** stehen mit ihren Regeln unter [Dateien und Medien](#dateien-und-medien-stufe-4).
  Keiner dieser Befehle ist `Never`; was nur Menschen dürfen (`path`, frei gewähltes Ziel), entscheidet der Wert
  (`Request::kept_for_people`). Abspielen von Sprachnachrichten und das Öffnen im Programm des Systems sind nur
  Oberfläche: Die Datei bekommt ein Client über `get_attachment`, abgespielt wird nur im Chatfenster bzw. im
  Programm des Systems.

## Anrufe (Stufe 6)

Entscheidung 13: selbst gebaut. Was die Machbarkeit dazu ergab, steht in
[chat-machbarkeit.md](chat-machbarkeit.md); hier steht, wie es in die Suite passt.

**Ablauf eines Anrufs:**
1. **Starten:** „Video call“ im Chatfenster schickt `chat.start_call` (nur App-Schlüssel). Der Kern tritt der
   Unterhaltung mit einer eigenen Sitzung bei (`POST /room/{token}/participants/active`, Cookie im Speicher des
   Kerns), dann dem Anruf (`POST /call/{token}`, `flags` Ton und Bild); Talk lässt die Gegenseite klingeln. Das
   Anruffenster öffnet sich.
2. **Klingeln:** Der Kern erkennt einen eingehenden Anruf (Entscheidung 9) und meldet ihn der App, die das
   kleine Fenster zeigt (Entscheidung 24). Ist `torrochat` nicht offen, startet der Kern es (dafür die Abfrage alle 10 s aus Entscheidung 9).
3. **Annehmen:** `chat.answer_call` mit oder ohne Bild; der Kern vergibt die Kennung `call` und gibt sie nur dem
   Anruffenster. Ablehnen (`chat.decline_call`) beendet nur das eigene Klingeln (der Kern nimmt die Benachrichtigung `call`
   zurück; `notification-state` fragt nur ab, ob weiter geklingelt wird; den genauen Weg prüft der Versuch), nicht den
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
  gerade nicht) oder eigene Wege nötig sind, klärt der Versuch. Eigene Bindungen an Kamera-Schnittstellen bräuchten `unsafe`; das gibt es heute nur in
  `torro-appkit`, und das ist nur für macOS. Für Linux oder Windows wäre eine neue Ausnahme nötig, also eine
  Rückfrage vor der Umsetzung.
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

**Stufe 6 in drei Schritten:**
- **(a) Versuch** (eigenes Issue, Code unter `spikes/` wie M9.4): ein Anruf zu zweit zwischen einem kleinen
  Rust-Programm und Talk-Web im Browser auf dem eigenen Server, auf Linux und macOS, mit Ton in beide Richtungen ohne
  Echo und Bild. Misst Aufbauzeit, CPU-Last, Größe des Programms und prüft TURN. Ergebnis als Abschnitt in
  [research.md](research.md). Scheitert er, entscheidet der Mensch neu (Browser oder später).
- **(b) Klingeln und Annehmen in der Suite:** kleines Fenster (Entscheidung 24), `get_call`, `decline_call`;
  „Accept“, „Audio call“ und „Video call“ öffnen das Gespräch zunächst im Browser (`<server>/call/<token>`). Der
  Kern tritt dann selbst keinem Anruf bei, sonst säße ein zweiter Teilnehmer ohne Bild und Ton darin; `start_call`
  und `answer_call` kommen erst mit (c).
- **(c) Anruffenster und Gerätewahl** nach den Entwürfen, mit Bild in Iced; danach öffnet „Accept“ das eigene
  Fenster.

**Umgesetzt in (b), Issue #643** (Kern und API; das Klingelfenster baut die App danach):
- `chat.get_call` liest „läuft“ und „seit wann“ aus der Liste der Unterhaltungen (`hasCall`, `callStartTime`) und
  die Teilnehmer aus `GET …/api/v4/call/{token}` (je Person einmal): ihre Zahl, und ob der Mensch dabei ist (seine
  Benutzer-ID unter ihnen, oder `participantFlags`). Die Antwort trägt `link` (`<server>/call/<token>`) für „Accept“.
  Clients fragen den Server einzeln nacheinander, wie bei Nachrichten.
- **Klingeln** ist eine Benachrichtigung von `spreed` mit dem Objekt `call`, jünger als 45 s, in einer Unterhaltung mit
  laufendem Anruf, in dem der Mensch auf keinem Gerät ist (`chat/calls.rs`). Die Abfrage läuft alle 10 s nur, solange
  `chat_ring` an ist; aus vergisst der Kern, was klingelt. Bei „Do not disturb“ auf dem Konto (eigener Status, wie
  zuletzt gelesen oder gesetzt) öffnet sich kein Fenster. Offen: `notify_push` statt der Abfrage und ein `ETag` je Konto
  für die Liste der Benachrichtigungen.
- **Ablehnen** löscht nur Benachrichtigungen, die noch klingeln; ein verpasster Anruf bleibt in den Benachrichtigungen
  und im Eingang. `notification-state` braucht es dafür nicht. **Auflegen** antwortet `ok`, solange der Anruf im
  Browser läuft.
- **Start des Fensters:** Für jeden Anruf, der neu klingelt, startet der Kern einmal `torrochat
  --ring=<unterhaltung>` (`apps::Opened::Ring`) und lässt die Klassen des Chats vorher schweben. Gewählt statt eines
  Ziels `ring:<unterhaltung>` für `apps.open_app`, weil ein solches Ziel jedem Client mit `apps` erlaubte, den Rechner
  klingeln zu lassen, und weil der Kern ohnehin selbst startet.
- Keiner der drei Befehle ist `Never`; die Datenregel des Assistenten hält `chat-call` wie jede Chat-Antwort zurück.

**Grenzen der ersten Fassung:** nur zu zweit (ohne HPB rät Talk selbst ab drei Personen ab), kein Bildschirm
teilen, keine Aufnahme, keine Untertitel. Föderierte Unterhaltungen und Gäste ohne Konto erst, wenn der Versuch
sie geprüft hat.

## Dateien und Medien (Stufe 4)

Mit #636. Was Talk und Nextcloud dafür bieten und was nicht (keine Dauer, Vorschaubilder von Videos nur mit `ffmpeg`
auf dem Server), steht in [chat-machbarkeit.md](chat-machbarkeit.md#dateien-und-medien-636). Entwürfe:
[prototypes/chat/](prototypes/chat/index.html#medien).

**Jede Funktion dreimal:**

| Oberfläche | API | MCP |
|---|---|---|
| Büroklammer, Ziehen, Einfügen, Beschriftung, ✕ vor dem Senden | `chat.prepare_file`, `chat.upload_file`, `chat.release_files` | `chat_prepare_file`, `chat_upload_file`, `chat_release_files` |
| Senden mit Datei | `chat.send_message` mit `files` | `chat_send_message` |
| Fortschritt, ✕ beim Senden | `chat.get_upload`, `chat.cancel_upload` | `chat_get_upload`, `chat_cancel_upload` |
| Vorschau im Verlauf, Bild groß, Raster der Medien-Übersicht | `chat.get_preview` | `chat_get_preview` |
| Open, Download, Video öffnen | `chat.get_attachment`, `chat.save_attachment` | `chat_get_attachment`, `chat_save_attachment` (nur Downloads oder ein Ablageziel) |
| Medien-Übersicht | `chat.list_shared` | `chat_list_shared` |
| Link-Karten | Feld `cards` in `chat.get_messages` | `chat_get_messages` |
| „Show once“ an einem fremden Link | `chat.show_card` | keins: nur Menschen (Entscheidung 23) |
| „Always allow …“ / „Never for …“ an einem fremden Link (#672) | `settings.set_setting privacy_provider` | keins: nur Menschen (`api.md`, Punkt 14) |
| Sprachnachricht abspielen (Datei über `get_attachment`, Wellenform im Fenster), Video im Programm des Systems | Abspielen selbst ist nur Oberfläche, ohne Befehl (Entscheidungen 19, 22) | – |
| „Show in chat“ aus der Medien-Übersicht | `chat.get_messages` mit `around` | `chat_get_messages` |

**Senden in zwei Schritten.** Vorbereiten und Senden sind getrennt, damit eine Freigabe genau die Bytes zeigt, die
später gehen:
1. **Vorbereiten:** `chat.prepare_file` bzw. `chat.upload_file` legen die Datei nur im Kern ab, als vorbereitete
   Datei mit Kennung (Lease), Name, Größe, Art, Prüfsumme (SHA-256; bei `path` stattdessen Größe und Änderungszeit)
   und kleiner Vorschau. Noch geht nichts zum Server.
   - `source: registered` nimmt nur eigene Dateien aus dem Register des Kerns, mit derselben Regel wie Anhänge in
     `mail.save_draft` ([api.md](api.md), Fußnote 22, „Eigene Werkzeuge“): ein Client nur solche, die er selbst
     erzeugt hat (sonst `not-found`), nur solange das Werkzeug eingeschaltet ist (`Lack::ToolOff`), er es in
     `Rights::tools` hat (`Lack::Tool`, beides `forbidden`) und die Datei unverändert ist (sonst `not-found`, wie
     `tools.open_file` bei einer geänderten Datei); geprüft
     vor der Freigabe und bei „Allow“ noch einmal.
   - `source: path` gibt es nur für den App-Schlüssel (Büroklammer, Ziehen), für Clients `forbidden`
     (Entscheidung 20). `prepare_file` liest dabei nur den Anfang der Datei (für die Art) und bei Bildern so viel,
     wie die Vorschau braucht; den ganzen Inhalt liest der Kern erst beim Senden und kopiert ihn vorher nicht. Er merkt
     sich Größe und Änderungszeit, und hat sich die Datei bis dahin geändert, antwortet das Senden `conflict`.
   - Eine Quelle `clipboard` gibt es nicht: Die Oberfläche lädt Eingefügtes selbst mit `upload_file` hoch.
   - `upload_file` folgt `mail.upload_image` ([api.md](api.md), „Eingebettete Mail-Bilder“) und gibt mit `final`
     dieselbe vorbereitete Datei wie `prepare_file`: Base64 in Stücken von höchstens 256 KiB (das IPC-Limit von 1 MiB bleibt), Beginn ohne Kennung bei Offset 0, gleiche Wiederholungen
     idempotent, andere `conflict`, nach `final` unveränderlich. Eine Art nimmt der Befehl nicht an, `name` ist Text
     des Clients. Höchstens 25 MiB je Datei, auch für den App-Schlüssel (Einfügen ist meist ein Bildschirmfoto;
     Größeres kommt über `path`), vier
     unfertige Uploads je Aufrufer, dieselben gemeinsamen Budgets im Speicher (Mangel ist `busy`); unfertige laufen
     nach 10 Minuten ab, fertige nach einer Stunde ohne Nutzung.
   - **Grenze je Datei:** für Clients 25 MiB; für den App-Schlüssel die kleinere aus dem freien Platz des Kontos
     (`quota.free` aus `GET /ocs/v2.php/cloud/user`; ein negativer Wert heißt unbegrenzt oder unbekannt, etwa bei
     Object Storage, und setzt keine Grenze) und 512 MiB. Darüber antwortet `prepare_file` mit `error`, bevor etwas
     hochgeladen wird, mit dem Grund, der greift: „This file is larger than {limit}.“ bzw. „There is not enough space
     in your Nextcloud.“ (Größen mit `format_bytes`).
   - **Die Art bestimmt der Kern** aus den ersten Bytes der Datei, nicht aus Name oder Angabe des Clients; nach ihr
     richten sich Freigabe, Vorschau und das Recht „Bildinhalte“.
   - `chat.release_files` verwirft eigene vorbereitete Dateien. Kennungen, die in einer wartenden Freigabe stehen,
     ergeben `conflict`, damit ein Client nicht unter einer Freigabe die Datei austauscht.
   - Dateien eines anderen Kontos als die Unterhaltung ergeben beim Senden `not-found`.
2. **Senden:** `chat.send_message { conversation, text, files }` ist der Befehl, der nach außen geht und ab Werk auf
   die Freigabe wartet. Erst bei seiner Ausführung, also nach „Allow“, lädt der Kern hoch und teilt:
   1. Anhangsordner aus `attachments.folder`, mit `conversation-subfolders` der Unterordner der Unterhaltung; ist
      `attachments.allowed` falsch, `unsupported`.
   2. Bis `files.chunked_upload.max_size` als ein `PUT` mit `If-None-Match: *`, darüber in Stücken (Chunking v2,
      nacheinander, je Stück `max_size`, aber mindestens 5 MiB und höchstens 5 GB, weil der Server kleinere Stücke
      außer dem letzten ablehnt). `max_size` ≤ 0 heißt: kein Chunking, immer ein `PUT`. Das abschließende `MOVE`
      trägt `Overwrite: F`, damit es eine vorhandene Datei nie ersetzt. Einen freien Namen („ (2)“, „ (3)“ … wie beim
      Teilen) wählt der Kern vorher per `PROPFIND`, weil `Destination` beim Chunking schon mit `MKCOL` feststeht;
      antwortet das `MOVE` trotzdem 412, prüft die erste Aufgabe der Stufe 4, ob ein neues Ziel geht oder der Upload
      neu beginnt.
   3. Freigabe Art 10 mit `talkMetaData`: `caption` = Text an genau einer Datei (an welcher, wie Talk-Web; prüft die
      erste Aufgabe der Stufe 4 gegen spreed), `silent`, `replyTo`, und je Datei eine `referenceId`, damit eine
      Wiederholung nach einem Abbruch der Verbindung nicht doppelt postet (ob Talk sie beim Teilen annimmt, prüft
      ebenfalls Stufe 4; ohne sie fragt der Kern vor einer Wiederholung `…/share` ab). Ohne `media-caption` erst der
      Text, dann die Dateien.
   4. Schlägt das Teilen fehl, löscht der Kern die eben hochgeladene Datei wieder (`If-Match` mit dem ETag des
      Uploads).
   - Die Antwort ist `pending` (Freigabe) oder, sobald der Upload beginnt, `chat-sending` mit nur der Kennung des
     Sendens, nie Name oder Pfad (Datenregel des Assistenten, Entscheidung 6). `chat.get_upload` mit dieser Kennung
     nennt Bytes, Stand (`uploading`, `sharing`, `sent`, `cancelled`, `failed` mit Code) und am Ende die Kennungen
     der entstandenen Nachrichten (je Datei eine). Ohne Dateien bleibt die Antwort `chat-sent` wie bisher.
   - **Abbrechen** (`chat.cancel_upload`, ✕ in der Oberfläche) bricht den Upload ab und löscht nur die eigene halbe
     Datei: den Upload-Ordner des Chunkings bzw. die Zieldatei mit `If-Match` auf den ETag, den der Kern selbst
     bekommen hat. Im Stand `sharing` antwortet er `conflict`, weil das Teilen auf dem Server schon unterwegs ist
     und sonst eine Nachricht mit gelöschter Datei entstünde. Was geteilt ist, bleibt; das ist dann Löschen einer
     Nachricht (Stufe 7).
   - Stufe 5 (Teilen) nimmt genau diesen Ablauf: Die Karte wird zu einer vorbereiteten Datei. Ebenso liefert
     Send to ([#260](https://github.com/mahype/TorroWorkplace/issues/260)) Dateien nur über `prepare_file`, keine
     eigene Tür in den Chat.

**Freigabe einer Datei.** Die Freigabe hält Kennung und Prüfsumme jeder Datei; der Kern prüft beides bei „Allow“
noch einmal, ebenso Rechte und Herkunft (`registered`). In `approvals.json` steht nur eine Hülle ohne Bytes, wie bei
Bildern der Mail: Die Bytes bleiben im Speicher, nach einem Neustart ist die Freigabe abgelaufen. Ist die Lease einer
Datei bis zur Entscheidung abgelaufen, antwortet die Ausführung `not-found`. Was die Freigabe zeigt, kommt aus
`approvals::Facts`, nie aus dem Text des Clients: Unterhaltung, Name (entschärft, 60 Zeichen), Größe, Art, Herkunft
(„made by this agent with the PDF tool“, „uploaded by this agent“), bei Bildern die Vorschau aus dem Kern, und
„Uploads <name> to your Talk folder in Nextcloud“; dazu die Beschriftung wie jeder Text. Die Art bestimmt der Kern
aus den Bytes. Ein Name aus `upload_file` ist Text des Clients und steht als solcher da („name given by this agent“).
Nach der Entscheidung leert der Kern Dateinamen und Bytes wie den Text (Datenschutz, „Wartende Freigaben“).

**Lesen und Öffnen.**
- **Nur Dateien der Nachricht:** `get_preview`, `get_attachment` und `save_attachment` prüfen `file` gegen die
  Dateien der Nachricht `id` im Cache des Kerns und antworten sonst `not-found`, bevor etwas geladen wird. Kennung
  und Pfad auf dem Server kommen nur aus dem Cache. Sonst öffnete `/core/preview?fileId=` bzw. WebDAV jedem Client
  mit `chat` den ganzen Speicher des Menschen (Test: fremde `fileId` → `not-found`).
- `chat.get_preview` holt `/index.php/core/preview` mit der `fileId` aus dem Cache (nie eine URL aus der Antwort),
  `small` 256 und `large` 1600 Pixel an der längeren Kante, `a=1`; mit `card: true` das Bild der Link-Karte über die
  Referenz-ID aus dem Cache der Nachricht, nie aus der Eingabe. Ohne `preview-available`, ohne Bild oder bei SVG
  `not-found`. Das MCP-Tool gibt das Bild als Bildinhalt zurück.
- **`hide-download`** gilt für jeden Weg: Bei so geteilten Dateien antworten `get_attachment` und `save_attachment`
  `forbidden`, für Clients wie für den App-Schlüssel; die Vorschau bleibt, „Open in Nextcloud“ auch.
- `chat.get_attachment` wie `mail.get_attachment`: Datei in den Anhangsordner des Chats (oben, Datenschutz), nach
  einem Tag gelöscht, Name bereinigt, Name und Inhalt `untrusted`; für Clients höchstens 50 MiB, für den
  App-Schlüssel 512 MiB, damit auch große Videos öffnen. Die Oberfläche öffnet den Pfad danach selbst mit dem
  Programm des Systems, wie `torroinbox` Mail-Anhänge (`xdg-open`, `open`); so öffnen auch Videos (Entscheidung 22).
  Größeres öffnet „Open in Nextcloud“.
- `chat.save_attachment` wie `mail.save_attachments`: `downloads` (Einstellung `mail_downloads_folder`, ein Ordner
  für Mail und Chat) oder ein Ablageziel; `to: file` und `to: folder` („Save as…“) nur mit dem App-Schlüssel,
  Clients `forbidden` (`kept_for_people`). Ein belegter Name wird nie überschrieben. Ablageziele kommen aus
  `mail.list_save_targets`; `save_attachment` mit `target` prüft im Dienst selbst Read in `mail` (sonst `forbidden`),
  nicht erst beim Nachschlagen der Kennung. Ein Client nur mit `chat` sichert nach `downloads`. Die
  Beschreibung des Tools sagt das.
- `chat.list_shared` liest `…/share` bzw. `…/share/overview` (Capability `rich-object-list-media`, sonst
  `unsupported`), `kind: link` aus den Karten im Cache, weil Talk Links nicht als Art führt. Einträge tragen Kennung
  der Nachricht und der Datei, Name, Größe, Art, Autor und Zeit, alles `untrusted`.
- **Link-Karten** stehen als Feld `cards` in `chat.get_messages` (Titel, Beschreibung, Domain, ob ein Bild da ist),
  `untrusted`. Von selbst löst der Kern nur Links auf den Ursprung des eigenen Kontos über `references/resolve` auf
  (Dateien, Deck, Talk), wenn er Nachrichten für ein offenes Chatfenster lädt (App-Schlüssel). Jeden anderen Link nach
  Settings › Privacy (#672, `torro_core::privacy`): von selbst bei „Always show“ und für einen erlaubten Anbieter
  (zusammen mit den eigenen höchstens fünf je Lesen), nie bei „Never show“ und für einen gesperrten – dann lässt der
  Kern auch eine schon gehaltene Karte weg, für jeden Leser –, sonst nennt er in der Nachricht den Anbieter (Feld
  `foreign`, nur für den App-Schlüssel), und die Karte kommt erst mit `chat.show_card { id }` („Show once“): Sonst
  riefe der eigene Server beim bloßen Anzeigen die fremde URL ab, und wer eine eindeutige URL schickt, erführe, wann
  gelesen wurde (Entscheidung 23). Die Einstellung liest der Kern bei jedem Lesen neu. Das Ergebnis hält der Kern im
  Cache. Ein Read eines Clients löst kein neues `resolve` aus, sonst könnte ein Agent den Server Links nachschlagen
  lassen. Das Bild der Karte kommt über
  `chat.get_preview` mit der Kennung der Nachricht und `card: true`.

**Rechte.**
- Alles gilt nur für Unterhaltungen in `chat_accounts`; fremde Kennungen (Nachricht, Datei, Upload, vorbereitete
  Datei eines anderen Clients) sind `not-found`. Ausnahme wie bei `mail.release_images`: `release_files` übergeht
  fremde und unbekannte Kennungen und antwortet `ok`, damit es nichts über fremde Dateien verrät.
- **Bildinhalte:** Für Clients brauchen `get_preview`, Bilder von Karten, `get_attachment` und `save_attachment` bei
  `image/*` und `video/*` zusätzlich `Rights.image_content` (aus dem Konzept der Fotos, [fotos.md](fotos.md),
  Entscheidung 4, noch nicht gebaut; kommt mit der ersten Aufgabe der Stufe 4, falls Fotos es bis dahin nicht
  gebracht haben), sonst `forbidden`. Sonst läse ein Agent ohne dieses Recht die Bilder einfach als Datei. Verlangt wird
  das Recht, sobald `mimetype` vom Server **oder** die Bytes auf Bild bzw. Video deuten **oder** die Art unbekannt ist
  (HEIC, RAW, ein fremder Container): Ein unbekanntes Format ist kein Weg am Recht vorbei. Geprüft wird im Dienst vor
  dem Herausgeben, nicht in `Rights::allows_request`, weil die Art am Request nicht steht: nach den Angaben im Cache vor
  dem Laden, nach den Bytes vor der Antwort. Ohne das Recht fehlt auch
  `blurhash` (ein winziges Abbild) in `get_messages` und `list_shared`. Der App-Schlüssel hat es immer.
- **Drossel:** Zu den 30 Nach-außen-Befehlen je Stunde (Datenschutz) kommen für Clients höchstens 20 Dateien und
  100 MiB je Stunde, sonst `busy`.
- **KI:** Antworten von `get_preview`, `get_attachment` und `list_shared` sind `chat.*` und gehen wie alles aus dem
  Chat nicht an Modelle anderswo (Entscheidung 6).

**Vorschlag für [api.md](api.md)**, wie beim vorgeschlagenen Punkt zu Anrufen (oben, Befehle), übernommen mit der ersten Aufgabe der Stufe 4 samt Test (Vorbild
`only_people_choose_where_attachments_go`) und den Werten in `kept_back` von `names_follow_the_rules`: In Punkt 10
(„Anhänge von Mails ablegen“) kommt dazu, dass auch `chat.prepare_file` mit `source: path` und
`chat.save_attachment` mit `to: file` oder `to: folder` nur mit dem App-Schlüssel gehen (`Request::kept_for_people`):
Den Pfad wählt ein Mensch im Dialog des Systems bzw. durch Ziehen. Sonst gäbe ein Agent dem Kern einen Pfad, und eine
Datei vom Rechner ginge mit nur einer Sende-Freigabe nach außen. Ebenso gibt es `chat.show_card` nur mit dem
App-Schlüssel (`Mcp::Never`): Ob der Absender eines fremden Links erfährt, wann gelesen wurde, entscheidet der Mensch
mit einem Klick, nicht ein Agent.

**Fenster:** Verlauf, Eingabe, Bild groß und Medien-Übersicht unter [Fenster](#fenster). Das neue Fenster
„Bild groß“ (Entscheidung 24) schwebt wie das Chatfenster (Entscheidung 16); die Medien-Übersicht ist eine
Seitenleiste im Chatfenster. Demo:
`torrochat --demo --open media | viewer | upload`.

**Umsetzung im Kern** (Backend der Stufe 4, `chat/media.rs`, `chat/attachments.rs`, `torro-nextcloud/src/files.rs`).
Wo der Code vom Text oben abweicht oder etwas offen ließ, gilt:
- **Beschriftung an der letzten Datei** einer Nachricht, `replyTo` ebenda; `referenceId` geht in `talkMetaData`, wenn
  der Server `chat-reference-id` nennt. Beides ist nicht gegen spreed und nicht gegen einen echten Server geprüft
  (Prüfung von Hand steht aus); nimmt Talk die `referenceId` beim Teilen nicht an, fragt der Kern vor einer
  Wiederholung noch nicht `…/share` ab.
- **412 beim `PUT` oder beim abschließenden `MOVE`:** Der Kern verwirft die Stücke, wählt per `PROPFIND` einen neuen
  freien Namen und beginnt den Upload neu, höchstens dreimal; danach `conflict`.
- **Eigenes Speicherbudget:** Vorbereitete Dateien und unfertige Uploads des Chats teilen sich 100 MiB, getrennt vom
  Budget der Mail-Bilder (eine gemeinsame Buchführung hätte `mail/images.rs` geändert); ein Client hält davon höchstens
  50 MiB, damit er das Einfügen der Oberfläche nicht sperrt. Eine Datei vom Rechner (`source: path`) zählt nicht mit,
  sie wird erst beim Senden gelesen.
- **Vorschauen brauchen immer `image_content`:** Eine Vorschau ist ein Bild des Inhalts, auch die Seite eines PDF.
  Darum verlangt `get_preview` das Recht für jede Datei, nicht nur bei `image/*` und `video/*`; `get_attachment` und
  `save_attachment` verlangen es wie oben nach der Art.
- **Drossel vor der Freigabe:** Ob die Dateien eines Clients noch in die 20 Dateien und 100 MiB je Stunde passen, prüft
  der Kern schon, bevor er fragt; gezählt wird erst beim Senden.
- **Stand `sharing` ab der ersten geteilten Datei:** Von da an bleibt das Senden bis zum Ende in `sharing`, und
  `cancel_upload` antwortet `conflict`, weil schon eine Nachricht in der Unterhaltung steht.
- **Leases werden beim Senden verbraucht:** Sobald das Hochladen beginnt, gibt es die vorbereiteten Dateien nicht mehr;
  scheitert das Senden, bereitet die Oberfläche sie neu vor. So kann dieselbe Datei nicht zweimal unter einer
  Freigabe gehen.
- **Abbrechen** wirkt zwischen zwei Stücken bzw. nach dem laufenden `PUT`: Der Kern löscht danach die Stücke bzw. die
  eben hochgeladene Datei mit `If-Match` auf ihren ETag.
- **Freien Platz** fragt `prepare_file` für den App-Schlüssel ab; ist der Server nicht erreichbar, gilt nur die
  Grenze von 512 MiB.
- **Ordner der geöffneten Anhänge** ist `<cache>/chat-attachments/<Prüfsumme der Kontokennung>/`, damit die Kennung
  (mit `@` und `:`) kein Pfadteil wird.
- **Freigabe:** Die Vorschau eines Bildes zeigt sie noch nicht (das Format der Freigaben kennt kein Bild); Name,
  Größe, Art und Herkunft stehen in den Details. Seit Stufe 4 stehen die Details einer Chat-Freigabe (ganzer Text,
  „Sent silently“, Dateien) auch wirklich an der Freigabe; zuvor berechnete der Kern sie, hängte sie aber nicht an.
- **Noch offen:** das Attribut `com.apple.quarantine` an geholten Dateien (macOS), die Dauer von Videos, Links in der
  Medien-Übersicht nennen Autor und Zeit nur, solange der Kern die Nachricht hält.

## Teilen in einen Chat (Stufe 5)

Übernimmt TL2 aus [teilen.md](teilen.md) und nutzt den Ablauf zum Senden aus
[Dateien und Medien](#dateien-und-medien-stufe-4) (Stufe 4):
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
     Agenten, sonst der Text der Karte, und eine `referenceId` (ob Talk sie beim Teilen einer Datei annimmt, prüft schon Stufe 4), damit eine Wiederholung nicht doppelt
     postet.
  4. Schlägt das Teilen fehl, löscht der Kern die eben hochgeladene Datei wieder (`If-Match`).
  - Ohne `media-caption` kommen zwei Nachrichten: erst der Text, dann die Datei.
  - Ohne Datei in der Karte geht nur der Text.
- **Größe:** höchstens 5 MB je Karte (die Grenze von `share_card`).
- **Umgesetzt (Issue #643):** nur Kontakte. Die Karte entsteht im Kern (`chat/share.rs`, `card_for`) und wird eine
  vorbereitete Datei des Aufrufers (Herkunft `Card`), die genau den Weg aus Stufe 4 nimmt; die Freigabe entsteht vor
  der Karte und liest Namen und Felder dafür selbst aus dem Adressbuch. Abweichend von oben: Die Datei heißt bei einem
  Kontakt nach ihm („Anna Muster.vcf“), erst bei mehreren `contacts.vcf`, damit die Gegenseite sie erkennt. Der Name geht
  immer mit; ein Foto geht noch nicht mit (die Auswahl kennt es nicht), ebenso kein Jahrestag. Höchstens 20 Inhalte je
  Nachricht; die Karte zählt zu den zehn Dateien einer Nachricht. Den Assistenten deckt der Kern ab: Ein geliehener
  Schlüssel ohne `contacts` hat das Recht nicht und bekommt `not-found`; eine eigene Zuordnung des Befehls zu
  `contacts` im Assistenten gibt es nicht. Das Teilen-Blatt fehlt, solange es keine Kontakte-App gibt.
- **Freigabe:** Unterhaltung, Text, jede Karte („Contact card: Anna Muster (6 fields)“, „+ private
  address“) und „Uploads contacts.vcf to your Talk folder in Nextcloud“.
- **Rechte:** „Send on my behalf“ (Entscheidung 4) plus Lesen der Inhalte. Das Schreiben in die
  Dateien des Menschen braucht kein eigenes Recht: Es ist Teil des Teilens und steht in der Freigabe.
- **Teilen-Blatt:** Ziel „Chat“, sobald ein Konto mit Talk da ist und der Aufrufer `chat` hat. Es
  zeigt die letzten Unterhaltungen (`chat.list_conversations`) mit Suche und ein Feld für einen
  eigenen Text.

## Offen (in der jeweiligen Stufe klären)

- **Antwort direkt in der Systembenachrichtigung:** macOS kann ein Textfeld in der Mitteilung
  (`UNTextInputNotificationAction`), das braucht AppKit-Code in `torro-appkit`. KDE kennt eine
  Erweiterung für Antworten in der Benachrichtigung, GNOME nicht. Eigener kleiner Plan nach Rückfrage. Anrufe
  brauchen das nicht: Sie klingeln in einem kleinen Fenster (Entscheidung 24).
- **Liste sparsamer holen:** Ob `GET /room?modifiedSince=…` auf allen unterstützten Versionen nur
  geänderte Unterhaltungen liefert, ist nicht gegen einen Server geprüft (der Server dieses Rechners hat kein Talk);
  bis dahin holt der Kern die ganze Liste, für Eingang und Chat in einer Anfrage.
- **Zahl der Teilnehmer im Kopf eines Gruppenchats:** Die Raumliste von Talk nennt sie nicht; sie je Gruppe einzeln
  abzufragen kostet bei jeder Liste eine Anfrage je Gruppe. Bis ein günstiger Weg geklärt ist, steht dort „Group chat“.

Erledigt mit Issue #643: Erledigen eines Talk-Eintrags braucht Write in `chat` mit dem Konto (vorher nur die
Quelle); geantwortet wird auf Talk-Einträge über `chat.send_message`, die Zeile in [eingang.md](eingang.md) ist
angepasst.
- **Teams als Gruppen in der Kontaktliste** (App `circles`) und eine Obergrenze für große Instanzen: Stufe 2.
- **„Do not disturb“ an einen Fokus-Modus oder eine Workspace-Vorlage koppeln:** später.
- **Gruppen anlegen, Teilnehmer, Unterhaltungen verlassen oder archivieren:** nicht in diesem Konzept.
- **Gruppenanrufe, Bildschirm teilen, Hintergrund weichzeichnen:** später, nach Stufe 6.
- **Sprachnachrichten aufnehmen** (Entscheidung 19): schaltet das Mikrofon ein; eigener Plan mit derselben Regel wie
  das Mikrofon des Assistenten ([api.md](api.md), „Was nie nach außen geht“, Punkt 6), nur Menschen.
- **Dauer von Videos:** Talk liefert keine. Ob der Kern sie aus dem Anfang der Datei lesen soll (eine Anfrage je
  Video), entscheidet Stufe 4 nach einem Blick auf die Kosten; bis dahin fehlt sie.
- **Video im Fenster abspielen** (GStreamer): eigenes Issue, wenn Videos im Alltag häufig sind (Entscheidung 22).
- **Tipp-Anzeige:** geht auch mit internem Signaling, kostet aber eine Sitzung und ein Long Polling je offenem
  Chatfenster. Kommt nach Stufe 6, wenn das Signaling für Anrufe ohnehin steht; mit HPB billiger.
- **TURN-Zugangsdaten im Anruffenster** (Datenschutz, Anrufe): Zustimmung des Menschen zur Ausnahme, erste Aufgabe
  der Stufe 6.
- **Medien im Fenster statt über den Kern** (Aufbau, Punkt 11): Zustimmung des Menschen zur Ausnahme von
  „Oberflächen lesen und schreiben nichts selbst“, erste Aufgabe der Stufe 6. Die Wahl „Selbst gebaut“ vom
  2026-11-06 setzt sie voraus (ohne sie gibt es kein eigenes Anruffenster); festgeschrieben in `api.md` wird sie
  erst nach ausdrücklicher Bestätigung.
- **Schweben unter AeroSpace und GlazeWM, Ort der Chatfenster unter Hyprland, ein Fenster je Unterhaltung über
  `apps.open_app`:** Stufe 3, jeweils mit Rückfrage, weil Schnittstellen von `torro-wm` bzw. `OpenApp` sich ändern.
- **Ist TURN auf dem eigenen Server eingerichtet?** Zeigt nur eine angemeldete Abfrage; klärt der Versuch.
- **Matrix oder Slack als weitere Anbieter:** Die Bereiche `chat` und `presence` und das Modell in `torro-chat`
  sind dafür offen. Ein eigener Plan, wenn es so weit ist.
