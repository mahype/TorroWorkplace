# Chat: App `torrochat` auf Nextcloud Talk

Stand: 2026-10-15. **Konzept, Entscheidungen vom 2026-10-15 eingearbeitet.** Aufgaben:
[tasks/chat.md](tasks/chat.md). Beantwortet die Idee „Chat (Nextcloud Talk)“ aus dem
[App-Katalog](app-catalog.md): **Wer hat mir geschrieben, und wie antworte ich schnell?**

## Ziel

Text-Chat der Suite auf **Nextcloud Talk**, in zwei Schritten, wie der Katalog es will:

1. **Schnell antworten, wo die Nachricht ankommt.** Ungelesene Unterhaltungen stehen schon heute im
   Notification Hub (Eingangsquelle `talk:<login>@<host>`, Aufgabe E5.9). Neu: Der Verlauf und ein
   Antwortfeld im Paneel, und „Reply“ an der Benachrichtigung.
2. **Eine schlanke Chat-App `torrochat`:** Unterhaltungen links, Verlauf rechts, Erwähnungen,
   Antworten auf eine Nachricht, neue Unterhaltung mit einer Person. **Nur Text, keine Anrufe**, kein
   Bildschirm teilen, keine Umfragen.

Dazu kommt das **Teilen in einen Chat** ([teilen.md](teilen.md), Stufe TL2): Kontakte und später
Notizen, Termine und Fotos als Karte mit Datei in eine Unterhaltung geben.

Wie jede Funktion gibt es den Chat dreimal: in der Oberfläche (Notification Hub und `torrochat`),
über die API (Bereich `chat`) und über MCP (`chat_*`). Ein Agent kann also „Was hat Anna im Team-Chat
zum Plan geschrieben?“ beantworten und eine Antwort vorschlagen. Schreiben ist **nach außen** und
wartet ab Werk auf eine Freigabe, wie Mail senden.

## Was Talk bietet

Talk (App `spreed`) spricht OCS unter `/ocs/v2.php/apps/spreed/api/…`, angemeldet mit Loginname und
App-Passwort wie alles andere. Funktionen erkennen wir über die Capabilities (`spreed.features`,
`spreed.config`), nicht über Versionsnummern. Quellen:
[Chat-API](https://nextcloud-talk.readthedocs.io/en/latest/chat/),
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
| Bearbeiten | `PUT /api/v1/chat/{token}/{id}`, nur 24 Stunden lang | `edit-messages` (Talk 19) |
| Löschen | `DELETE /api/v1/chat/{token}/{id}`, nur 6 Stunden lang | `delete-messages` |
| Reaktionen | `POST`/`DELETE /api/v1/reaction/{token}/{id}` mit `reaction` | `reactions` |
| Datei teilen | Datei per WebDAV in den Anhangsordner, dann `POST /ocs/v2.php/apps/files_sharing/api/v1/shares` mit `shareType=10`, `shareWith=<token>`, `path`, `talkMetaData={"caption": …}` | `media-caption` (Talk 18) |
| Anhangsordner | `spreed.config.attachments.folder` (je Nutzer, meist `/Talk`), `attachments.allowed` | – |
| Höchstlänge | `spreed.config.chat.max-length` (heute 32.000 Zeichen) | – |
| 1:1 anlegen | `POST /api/v4/room` mit `roomType=1`, `invite=<user-id>`; gibt die vorhandene zurück | – |

**Eigenheiten, auf die wir achten:**
- **Lesen ändert nichts.** Ab Werk setzt das Lesen von Nachrichten den Lesestand und löscht
  Benachrichtigungen. Der Kern schickt darum immer `setReadMarker=0`, auf Servern mit
  `chat-keep-notifications` auch `markNotificationsAsRead=0`, und `noStatusUpdate=1`, damit Abfragen
  den Menschen nicht „online“ zeigen. Gelesen wird nur ausdrücklich (`chat.mark_read`).
- **Text mit Platzhaltern:** `message` enthält `{mention-user1}`, `{file}` usw., die Werte stehen in
  `messageParameters`. Ersetzt wird mit dem vorhandenen `rich::plain`. Dateien und Erwähnungen gibt
  es zusätzlich als eigene Felder, damit die App sie als Chips zeigen kann.
- **Systemnachrichten** („Anna started a call“) haben `messageType: system`. Die App zeigt sie klein,
  manche gar nicht (`reaction`, `message_edited`: Sie ändern nur eine andere Nachricht).
- **Antworten auf Bearbeiten und Löschen** sind selbst Systemnachrichten. Sie ersetzen die Nachricht
  im Cache und werden nicht gezeigt.
- **Lobby und schreibgeschützte Unterhaltungen** antworten mit 412 bzw. 403.
- **Föderierte Unterhaltungen** (Teilnehmer auf anderen Servern) gehen mit `federation-v1` wie
  andere. Ohne die Capability lesen wir sie nur aus der Liste der Unterhaltungen.
- **Versionen:** Nextcloud unterstützt die letzten drei Hauptversionen; alle bringen Talk 18 oder
  neuer mit. `media-caption` ist also überall da. Fehlt eine Capability trotzdem, fehlt die Funktion
  in der Oberfläche, und der Befehl antwortet `unsupported`.

## Entscheidungen (2026-10-15)

1. **Name:** App `torrochat`, Bereich `chat`, MCP-Tools `chat_*`, Fensterklasse `torro-chat`. Der
   Bereich ist neutral wie `contacts` und `notes`: Talk ist der erste Anbieter, Matrix oder Slack
   könnten folgen. Die Eingangsquelle heißt weiter `talk:<login>@<host>`, sie ist eine Quelle des
   Eingangs, kein Bereich. Der Befehl aus TL2 heißt darum **`chat.send_message`** (statt
   `talk.send_message`), MCP `chat_send_message`.
2. **Crates:**
   - `torro-chat` (neu): neutrales Modell (Unterhaltung, Nachricht, Autor, Erwähnung, Datei),
     Anbieter-Trait, Quelle für die App, Demo. Ohne Ein-/Ausgabe, Vorbild `torro-pim`.
   - Talk kommt in das bestehende `torro-nextcloud` (`talk.rs` wächst um Nachrichten und Senden), das
     Hochladen und Teilen von Dateien nach `files.rs` (neu).
   - Der Kern-Teil kommt nach `crates/torroworkplace/src/chat/`.
   - `torrochat` (neu) ist die App.
3. **Rechte je Konto**, wie bei Mail: `Rights.chat_accounts: Selection` mit den Kennungen der
   Nextcloud-Konten (`nextcloud:<login>@<host>`). Fehlt in alten Rechten = nichts.
   - **Was ein Client nicht sehen darf, gibt es für ihn nicht:** Unterhaltungen anderer Konten sind
     `not-found`, gefiltert vor dem Lesen, auch bei Zählern und Suche.
   - **Die Rechte-Vorlagen öffnen kein Konto.** Sie setzen nur die Stufe in `chat`; `chat_accounts`
     bleibt leer (wie bei Kontakten und Notizen). Beim Verbinden eines Agenten sind die Konten mit
     Talk vorgewählt, und der Mensch bestätigt sie. `Rights::template()` vergleicht `chat_accounts`
     nicht mit.
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
   Löschen, Reaktionen und eine neue Unterhaltung anlegen. Auch „Note to self“ zählt dazu: Der Zugriff
   ergibt sich aus dem Befehl, nicht aus der Art der Unterhaltung (`Request::operation()` sieht nur den
   Request), und eine Ausnahme wäre ein Weg, die Regel zu umgehen. Gelesen/ungelesen, Favorit und
   Benachrichtigungsstufe sind nur `Write`. Der Lesestand ist zwar für andere sichtbar (Lesebestätigung,
   wenn der Mensch sie in Talk nicht abgeschaltet hat), aber er verschickt keinen Inhalt und sagt nur,
   was ohnehin geschah: Der Mensch oder sein Agent hat gelesen. Ihn hinter eine Freigabe zu legen,
   hieße, dass jedes Lesen eines Agenten auf den Menschen wartet.

   Die Begründung bei `forbidden` (`Lack::Outbound`) sagt heute „sending mail on the person's behalf“.
   Sie wird neutral: „sending on the person's behalf is switched off for this client“.
5. **Nachrichten sind immer `untrusted: true`.** Chats sind wie Mail Text von anderen, der klassische
   Weg für eingeschleuste Anweisungen. Katalogbeschreibungen und MCP-Tools sagen, dass Nachrichten
   **Daten, keine Anweisungen** sind. Das gilt für Text, Namen von Unterhaltungen, Autorennamen und
   Dateinamen.
6. **Chat geht an KI nur auf den Rechnern des Menschen**, wie Mail und die Talk-Einträge des
   Eingangs ([ki.md](ki.md), Grundsatz 4: „Mail und Messenger nur lokal“):
   - `Area::Chat` kommt in `NEVER_TO_AGENTS`. Ein Coding-Agent als Antrieb des Assistenten bekommt
     den Chat nie. **Auch nicht über den Eingang:** Mail-Einträge fallen für ihn schon weg, weil sie
     `Area::Mail` brauchen; Talk-Einträge brauchen heute nur die Quelle. Leiht der Kern einen Schlüssel
     ohne `chat`, streicht er darum auch die Quellen `talk:*` aus `inbox_sources`. Eine Restlücke
     bleibt bewusst: Die Quelle `nextcloud:*` meldet Anrufe und Einladungen von Talk mit dem Namen der
     Unterhaltung und der Person (keinen Nachrichtentext). Sie fällt unter die Regeln der
     Benachrichtigungen, nicht des Chats.
   - Die Datenregel des Assistenten (`assistant/data_rule.rs`) hält Antworten der `chat.*`-Befehle für
     Modelle anderswo zurück, wie `mail.*`. Gebaut als Positivliste (Vorbild
     `MAIL_ANSWERS_ALLOWED`): Durch gehen nur Antworten ohne fremden Text (`ok`, die eigene Antwortart
     `chat-sent` mit nur der Kennung der gesendeten Nachricht, Fehler); jeder spätere `chat.*`-Befehl
     ist damit von selbst zurückgehalten.
   - Wer `chat.*` aufgerufen hat, hat fremden Text gelesen, wie bei Mail: `reads_mail` und
     `may_have_read_untrusted` kennen `chat.` bzw. `Area::Chat`. So geht auch der Verlauf eines
     Gesprächs danach nicht an ein Modell anderswo, und eine Notiz des Assistenten wird `local_only`.
   - `Rights::assistant()` bekommt `chat` nicht. Der Mensch schaltet es in „What the assistant may
     use“ ein und wählt die Konten.

   Externe Agenten mit eigenem Schlüssel (Claude Code über MCP) sehen den Chat, wenn der Mensch es
   ihnen gibt; das ist dieselbe Grenze wie bei Mail.
7. **Kein Verlauf auf der Platte.** Der Kern hält Nachrichten nur im Speicher: die letzten 200 je
   Unterhaltung, die in den letzten 30 Minuten gelesen wurde, höchstens 20 Unterhaltungen. Talk ist
   ein Online-Dienst, der Verlauf liegt auf dem Server, und ein zweites Archiv privater Chats auf dem
   Rechner wäre ein unnötiges Risiko. Ohne Netz zeigt die App, was im Speicher ist, und sagt
   „Offline“.
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
     hält der Kern eine Long-Polling-Anfrage offen, höchstens drei zugleich. Apps fragen den Kern alle
     2 s mit `after`; das ist ein Blick in den Speicher. Clients merken nichts vor: Sie bekommen den
     Cache, und ist er älter als 10 s, holt der Kern einmal neu, je Client höchstens eine solche Abfrage
     zur Zeit (sonst nur der Cache). So kann kein Agent die Plätze der Oberfläche belegen oder den
     Server mit Abfragen über viele Unterhaltungen beschäftigen.
   - Push über `notify_push` meldet Talk-Nachrichten nicht eigens. Ob die Benachrichtigungen dort
     (`notify_notification`) die Liste früher anstoßen können, prüft CH1.3.
10. **Schnell antworten im Notification Hub** (Stufe CH1), nicht im Tray. Das Tray zählt weiter nur
    bei „Inbox (3)“ und listet keine einzelnen Meldungen (Regel aus E3.3b). Die Benachrichtigung eines
    Talk-Eintrags bekommt den Knopf „Reply“. Er öffnet den Notification Hub beim Eintrag mit dem
    Cursor im Antwortfeld. Eine Antwort direkt in der Systembenachrichtigung braucht nativen Code je
    System und ist offen (siehe unten).
11. **Teilen in einen Chat (TL2)** gehört zu diesem Plan, Stufe CH3. `chat.send_message` bekommt
    `items` und `fields` wie `mail.save_draft` in TL1. Der Kern baut die Karte über `share::card_for`
    mit den Rechten des Aufrufers, lädt die Datei in den Anhangsordner und teilt sie mit dem Text der
    Karte als Beschriftung. Mehr unter [Teilen in einen Chat](#teilen-in-einen-chat-ch3).
12. **Stufen:**
    - **CH1:** Kern, API, MCP und schnell antworten im Notification Hub.
    - **CH2:** die App `torrochat`.
    - **CH3:** Teilen in einen Chat (TL2).
    - **CH4:** Pflege: eigene Nachrichten bearbeiten und löschen, Reaktionen, Favoriten,
      Benachrichtigungsstufe, Suche in Nachrichten.

    Jede Stufe ist für sich nutzbar und über alle drei Wege erreichbar.

## Aufbau

```
torroinbox (Paneel, CH1) ─┐
torrochat (App, CH2)  ────┼─ IPC ─▶ torroworkplace (Kern)
torrocontacts (Teilen) ───┘          ├─ chat/service.rs   Befehle, Rechte-Filter, Freigabe-Fakten, Drossel
                                     ├─ chat/cache.rs     Unterhaltungen und Nachrichten im Speicher
                                     ├─ chat/live.rs      Long Polling der vorgemerkten Unterhaltungen
                                     ├─ chat/share.rs     Karte → Datei hochladen → teilen (CH3)
                                     └─ sources/nextcloud.rs, sources/talk.rs  Liste alle 60 s (gibt es)
                                              │
                          torro_chat::ChatProvider (Trait, Modell)
                             └─ torro-nextcloud: talk.rs (Unterhaltungen, Nachrichten), files.rs (Upload, Freigabe)
```

**Kennungen:**
- **Konto:** die Kennung des Nextcloud-Kontos, `nextcloud:anna@cloud.example`.
- **Unterhaltung:** `<konto-id>/<token>`, zum Beispiel `nextcloud:anna@cloud.example/x7k2m9qa`.
  Getrennt am letzten `/`; der Token muss `RoomToken::parse` bestehen.
- **Nachricht:** `<unterhaltung>/<nachricht-id>`, zum Beispiel
  `nextcloud:anna@cloud.example/x7k2m9qa/5122`.
- Ein Eintrag der Quelle `talk:<x>` im Eingang führt seine Unterhaltung als neues Feld
  `conversation` (CH1.1).

**Fehler** (feste Codes):

| Fall | Code |
|---|---|
| Server nicht erreichbar | `offline` |
| 401 | `auth`, danach Remote-Wipe-Prüfung wie bei CalDAV |
| Talk fehlt auf dem Server | `not-connected` |
| Unterhaltung weg, Mensch nicht mehr Teilnehmer, fremdes Konto | `not-found` |
| schreibgeschützt (403), Lobby (412), Funktion fehlt (Capability) | `unsupported` mit Grund („This conversation is read-only.“) |
| Text zu lang (413 oder über `max-length`) | `error` |
| Bearbeiten nach 24 h, Löschen nach 6 h (400) | `unsupported` („Messages can only be edited for 24 hours.“) |
| 429, Drossel des Kerns | `busy` |

`message` ist immer ein fester englischer Text des Kerns. Meldungen aus der Antwort von Talk landen nie
roh darin: Sie sind fremder Text und kämen sonst ungeprüft zu einem Agenten.

**Sicherheit:**
- **Kein Inhalt im Protokoll:** Befehl, Konto und Prüfsummen der Kennungen (`audit::ids`). Texte,
  Namen, Tokens und Suchbegriffe stehen dort nie.
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
  - bei Bearbeiten „alt → neu“, bei Löschen den Anfang der Nachricht, bei Reaktionen das Emoji;
  - bei Karten (CH3) jede Karte wie bei Mail und dass eine Datei in den Talk-Ordner geladen wird.

  Namen gehen durch dieselbe Entschärfung wie in anderen Freigaben (eine Zeile, 60 Zeichen). Kennt
  der Cache eine Unterhaltung nicht mehr, steht dort „unknown conversation“.
- **Wartende Freigaben auf der Platte:** Bis der Mensch entscheidet, liegt der Request mit dem Text in
  der Datei der Freigaben (0600). Das ist die einzige Stelle, an der Chat-Text auf die Platte kommt
  (Entscheidung 7). Heute hält der Kern entschiedene und abgelaufene Freigaben noch einen Tag samt
  Request; bei `chat.*` leert er den Text darum sofort bei der Entscheidung oder dem Ablauf.
- **Drossel:** Ein Client (gezählt je Client-Kennung, damit geliehene Schlüssel des Assistenten sie
  nicht umgehen) führt höchstens 30 Nach-außen-Befehle in `chat` je Stunde und 5 je Minute aus
  (Senden, Bearbeiten, Löschen, Reaktionen, neue Unterhaltung zusammen), sonst `busy`. Auch mit „Free“
  kann ein entgleister Agent so niemanden zuschütten. Der App-Schlüssel ist nicht gedrosselt.
- **Erwähnungen von allen** (`@all`) sind für Clients nicht verboten, stehen aber in der Freigabe.
- **Links und Dateien:** Dateien in Nachrichten zeigt die App mit Name, Größe und „Open in
  Nextcloud“. Der Link wird aus der Kennung der Datei auf dem Ursprung des Kontos gebaut
  (`<server>/f/<id>`), nie aus der Antwort übernommen (Regel 15 aus E5). Links im Text öffnen sich nur
  nach einem Klick und nur mit `https`, `mailto`. Bilder lädt der Verlauf nicht (kein Verfolgen,
  nur Text).
- **Konto weg oder Remote Wipe:** Der Cache des Kontos wird geleert, laufende Long-Polling-Anfragen
  werden abgebrochen, das Konto fällt aus allen `chat_accounts`.

## Befehle (Bereich `chat`)

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| CH1 | `chat.list_conversations { account?, unread_only?, query?, limit? }` (Name, Art, ungelesen, Erwähnung, letzte Nachricht) | Read | `chat_list_conversations` |
| CH1 | `chat.get_messages { conversation, before?, after?, limit? }` (höchstens 100, ändert keinen Lesestand) | Read | `chat_get_messages` |
| CH1 | `chat.send_message { conversation, text, reply_to?, silent? }` | Write + **nach außen** | `chat_send_message` |
| CH1 | `chat.mark_read { conversation, up_to? }` | Write | `chat_mark_read` |
| CH1 | `inbox.get_item` mit dem neuen Feld `conversation` bei Talk-Einträgen (Erweiterung) | Read (`inbox`) | `inbox_get_item` (vorhanden) |
| CH1 | `apps.open_app` mit `target: "item:<id>"` (Notification Hub beim Eintrag, Antwortfeld, Erweiterung) | Write (`apps`) | `apps_open_app` (vorhanden) |
| CH2 | `chat.mark_unread { conversation }` | Write | `chat_mark_unread` |
| CH2 | `chat.suggest_mentions { conversation, query, limit? }` | Read | `chat_suggest_mentions` |
| CH2 | `chat.find_people { account, query, limit? }` (Personen des Servers für eine neue Unterhaltung) | Read | `chat_find_people` |
| CH2 | `chat.create_conversation { account, with }` (1:1, gibt eine vorhandene zurück) | Write + **nach außen** | `chat_create_conversation` |
| CH2 | `apps.open_app` mit `target: "chat:<unterhaltung>"` (Erweiterung) | Write (`apps`) | `apps_open_app` (vorhanden) |
| CH3 | `chat.send_message` mit `items` und `fields` (Erweiterung, TL2) | Write + **nach außen** + Read der Inhalte | `chat_send_message` (vorhanden) |
| CH4 | `chat.edit_message { id, text }` | **Delete** + **nach außen** | `chat_edit_message` |
| CH4 | `chat.delete_message { id }` | Delete + **nach außen** | `chat_delete_message` |
| CH4 | `chat.react { id, emoji, remove? }` | Write + **nach außen** | `chat_react` |
| CH4 | `chat.set_favorite { conversation, favorite }` | Write | `chat_set_favorite` |
| CH4 | `chat.set_notification_level { conversation, level }` (`default`, `always`, `mention`, `never`) | Write | `chat_set_notification_level` |
| CH4 | `chat.search_messages { query, account?, limit? }` (Unified Search von Talk) | Read | `chat_search_messages` |

Erläuterungen:
- **Kein Befehl steht auf `Never`.** Nichts davon fällt unter
  [„Was nie nach außen geht“](api.md#was-nie-nach-außen-geht). Was andere sehen, läuft über
  „Send on my behalf“ und wartet ab Werk auf eine Freigabe.
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

## Oberfläche

**Notification Hub (CH1)**, Paneel eines Talk-Eintrags:
- **Oben** wie heute Titel (Unterhaltung) und Quelle.
- **Verlauf:** die letzten 20 Nachrichten, die ungelesenen hinter einem Trenner „New“. Ältere über
  „Show earlier“. Systemnachrichten klein und grau.
- **Antwortfeld unten:** Enter sendet, Shift+Enter bricht um. „Reply“ an einer Nachricht zitiert sie
  über dem Feld. Nach dem Senden markiert das Paneel den Eintrag als erledigt (`inbox.set_item_done`,
  das setzt in Talk schon heute den Lesestand).
- **Ohne Recht auf `chat`** steht statt Verlauf und Feld nur die letzte Zeile und „Open in Talk“.
- **„Open in Chat“** (ab CH2) öffnet `torrochat` bei der Unterhaltung, sonst „Open in Talk“ im Browser.
- **Benachrichtigung:** Knopf „Reply“ neben „Done“ und „Snooze 1 h“.

**`torrochat` (CH2)** nutzt `torro_gui::shell`:
- **Seitenleiste:** Suchfeld, Filter „All“ und „Unread“, darunter die Unterhaltungen nach letzter
  Aktivität: Bild (Initialen), Name, letzte Zeile, Zeit, Zahl der ungelesenen, „@“ bei Erwähnung.
  Favoriten oben, „Note to self“ darunter. Ab zwei Konten je Konto gruppiert. Knopf „New chat…“.
- **Verlauf:** Nachrichten nach Tag und Autor gruppiert, eigene rechts. Zitat bei Antworten,
  Erwähnungen hervorgehoben, Dateien als Zeile mit Symbol, Name, Größe und „Open“. Einfaches Markdown
  (fett, kursiv, Code, Links), keine Bilder. Trenner „New messages“ beim Lesestand. Nach oben
  scrollen lädt ältere.
- **Eingabe:** mehrzeilig, Enter sendet, Shift+Enter bricht um. `@` öffnet Vorschläge, Pfeiltasten
  und Enter wählen. Ein Zähler erscheint ab 90 % der Höchstlänge. Im Menü „Send without
  notification“.
- **Gelesen:** Ist das Fenster im Vordergrund und die neueste Nachricht sichtbar, setzt die App den
  Lesestand. Kontextmenü „Mark as unread“.
- **„New chat…“:** Personensuche (`chat.find_people`), Enter öffnet oder legt die 1:1-Unterhaltung an.
- **Tasten:** `⌘K`/`Ctrl+K` springt zu einer Unterhaltung, `Alt+↑`/`Alt+↓` zur nächsten ungelesenen,
  `Esc` bricht eine Antwort ab.
- **Schmal** zeigt das Fenster entweder die Liste oder den Verlauf.
- **Mini-Kachel:** die ungelesenen Unterhaltungen mit Zahl, ein Klick öffnet sie.
- **Kein Talk-Konto:** ein leerer Zustand mit „Connect Nextcloud“, der die Steuerzentrale öffnet.
  Konto ohne Talk: „Talk is not installed on this server.“
- **Demo:** `torrochat --demo` mit drei Unterhaltungen, Erwähnung, Datei und Systemnachricht.

**Steuerzentrale:** Rechte-Zeile „Chat“ mit Kontoauswahl; der Hinweis an „Send on my behalf“ nennt
künftig Mail, Antworten auf Einladungen und Chat. Auf der Kontoseite steht Talk bei den Diensten.

**Teilen-Blatt (CH3):** Ziel „Chat“ mit den letzten Unterhaltungen und Suche, siehe unten.

## Teilen in einen Chat (CH3)

Übernimmt TL2 aus [tasks/teilen.md](tasks/teilen.md):
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

## Offen (in der jeweiligen Aufgabe klären)

- **Antwort direkt in der Systembenachrichtigung:** macOS kann ein Textfeld in der Mitteilung
  (`UNTextInputNotificationAction`), das braucht AppKit-Code in `torro-appkit`. KDE kennt eine
  Erweiterung für Antworten in der Benachrichtigung, GNOME nicht. Eigener kleiner Plan nach Rückfrage.
- **`inbox.reply`** aus [eingang.md](eingang.md) war als ein Antworten für Mail, Slack und Talk
  geplant. Für Talk antwortet das Paneel jetzt über `chat.send_message`. CH1.1 passt die Zeile in
  [eingang.md](eingang.md) an; ob Mail und Slack ein gemeinsames `inbox.reply` brauchen, entscheiden
  deren Pläne.
- **Liste sparsamer holen:** Ob `GET /room?modifiedSince=…` auf allen unterstützten Versionen nur
  geänderte Unterhaltungen liefert, prüft CH1.2.
- **Gruppen anlegen, Teilnehmer, Unterhaltungen verlassen oder archivieren:** nicht in diesem Plan.
- **Bilder im Verlauf** (Vorschau über `/core/preview`) und **Dateien hochladen** außer über Teilen:
  nicht in diesem Plan, „nur Text“.
- **Tipp-Anzeige und Online-Status** laufen in Talk über das Signaling, nicht über OCS. Nicht in
  diesem Plan.
- **Matrix oder Slack als weitere Anbieter:** Der Bereich `chat` und das Modell in `torro-chat` sind
  dafür offen. Ein eigener Plan, wenn es so weit ist.
