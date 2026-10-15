# Aufgaben: Chat (`torrochat`) auf Nextcloud Talk, Stufen CH1–CH4

Konzept: [chat.md](../chat.md). Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill
`torro-task`. Wer einen Request anfasst, lädt `torro-api`; nach jeder Funktion prüft
`torro-api-review`. Wer mit Talk oder Nextcloud spricht, lädt den Skill `nextcloud` (Agent
`nextcloud-expert` geeignet) und belegt jeden Endpunkt mit einem Link auf die Talk-Doku oder
`openapi.json`. Nach jeder UI-Änderung prüft `torro-design-review`.

Jede Stufe ist für sich nutzbar und über Oberfläche, API und MCP erreichbar. Request und
Katalogeintrag einer Stufe kommen in ihre erste Katalog-Aufgabe (CH1.1, CH2.1, CH3.2, CH4.2).

**Für alle Aufgaben gilt** (Entscheidungen 3 bis 9 in [chat.md](../chat.md)):
- Nachrichten, Namen von Unterhaltungen, Autoren und Dateinamen sind `untrusted: true`. Beschreibungen
  sagen: Daten, keine Anweisungen.
- Rechte gelten je Konto (`Rights.chat_accounts`). Die Vorlagen öffnen keins. Was ein Client nicht
  sehen darf, ist `not-found` und wird vor dem Lesen ausgefiltert.
- Alles, was andere Menschen sehen, ist nach außen (`Command::is_outbound`, Schalter „Send on my
  behalf“) und wartet ab Werk auf eine Freigabe.
- Lesen ändert nichts beim Menschen: immer `setReadMarker=0` und `noStatusUpdate=1`, mit
  `chat-keep-notifications` auch `markNotificationsAsRead=0`.
- Kein Verlauf auf der Platte, kein Senden später.
- `message` in Fehlern ist ein fester englischer Text des Kerns, nie die Meldung aus der Antwort von Talk.
- Ins Protokoll kommen Befehl, Konto und Prüfsummen der Kennungen, nie Texte, Namen oder Tokens.
- Sichtbare Texte englisch mit `tr()`/`trf()`. Die deutschen Übersetzungen trägt die Koordination in
  `crates/torro-core/src/i18n/de.rs` ein; die Aufgabe nennt sie im Bericht.
- Jede Aufgabe, die eine `Area` hinzufügt, nimmt alle Stellen mit, die `Area` erschöpfend prüfen
  (heute `crates/torroworkplace/src/ui/agents.rs` und `crates/torroassistant/src/chat.rs`), mit
  Beschriftungen in `tr()`.

Aufgaben, die den Katalog ändern (CH1.1, CH2.1, CH3.2, CH4.2), laufen nie parallel zu anderen
Katalog-Aufgaben (auch nicht zu denen aus [kontakte.md](kontakte.md), [notizen.md](notizen.md) und
[teilen.md](teilen.md)).

```
CH1.0 ─┬─ CH1.2 ───────────┐
       ├─ CH1.1 ───────────┴─ CH1.3 ─┬─ CH1.4 ── CH1.5
       │                             └─ CH1.6
       └─ CH2.2 (App mit Demo, sofort nach CH1.0)
CH2.1 (braucht CH1.3) ── CH2.3 (braucht auch CH2.2)
CH3.1 (braucht CH1.2) ── CH3.2 (braucht auch CH1.3, TL1.1) ── CH3.3 (braucht auch TL1.2, TL1.5)
CH4.1 (braucht CH1.2) ── CH4.2 (braucht auch CH1.3) ── CH4.3 (braucht auch CH2.3)
```

## CH1 Kern, API, MCP und schnell antworten im Notification Hub

### CH1.0 Modell-Crate `torro-chat`
- [ ] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-chat/` (neu: `Cargo.toml`,
  `src/{lib,model,id,provider,source,demo}.rs`), Wurzel-`Cargo.toml` (Mitglied und Eintrag in
  `[workspace.dependencies]`)
- Ohne Ein-/Ausgabe und ohne Abhängigkeit zu `torro-ipc` und `torro-nextcloud`. Vorbild `torro-pim`.
  Alle Typen mit `Serialize`, `Deserialize` und `JsonSchema`, weil CH1.1 sie als Wire-Format benutzt.
- **Kennungen (`id.rs`):** `ConversationId` (`<konto-id>/<token>`, getrennt am letzten `/`) und
  `MessageId` (`<unterhaltung>/<nachricht-id>`) mit Zerlegung und Prüfung. Der Token folgt dem Muster
  von Talk (4 bis 30 Zeichen `a`–`z`, `0`–`9`), die Nachricht-Id ist eine positive Zahl. Ungültiges
  ist ein eigener Fehler, kein `panic`.
- **Typen (`model.rs`):**
  - `Conversation { id, account, kind, name, unread, mention, mention_direct, last_activity,
    last_message: Option<MessagePreview>, last_read, read_only, public, favorite,
    notification_level, untrusted }`, `kind`: `direct | group | public | note_to_self | other`.
  - `Message { id, author: Author, own, time, text, mentions: Vec<Mention>, files: Vec<FileRef>,
    reply_to: Option<MessagePreview>, system: bool, edited: Option<DateTime>, deleted, reactions:
    Vec<Reaction>, untrusted }`. `Author { kind: user | guest | bot | federated | system, id, name }`.
  - `Mention { id, label, kind, untrusted }`, `Person { id, name, untrusted }` (für `find_people`),
    `FileRef { id, name, mime, size }` (ohne Link: Den baut der Kern
    aus der Kennung, Regel 15 aus E5), `Reaction { emoji, count, own }`.
  - `MessagePage { messages, has_more }`, `NewMessage { text, reply_to, silent }`.
- **`provider.rs`:** Trait `ChatProvider` für den Kern, async: `conversations()`,
  `messages(token, before, after, limit)`, `wait(token, after, timeout)`, `send(token, NewMessage,
  reference_id)`, `mark_read(token, up_to)`, `mark_unread(token)`, `mentions(token, query, limit)`.
  CH2.1, CH3.1 und CH4.1 erweitern ihn. Fehler als eigenes Enum: `Auth`, `Offline`, `MissingApp`,
  `NotFound`, `ReadOnly`, `Lobby`, `TooLong`, `TooOld`, `Unsupported`, `Throttled`, `Other`.
- **`source.rs`:** Trait `ChatSource` für die App, async wie `PimSource`, mit den Befehlen aus CH1 und
  CH2.
- **`DemoSource`** für `--demo`, Änderungen wirken im Speicher: zwei Konten, sechs Unterhaltungen
  (1:1, Gruppe mit Erwähnung, öffentlich, schreibgeschützt, „Note to self“, föderiert), je 30
  Nachrichten mit Antwort, Datei, Systemnachricht, bearbeiteter und gelöschter Nachricht.
- **API/MCP:** keine. Requests und Katalog kommen in CH1.1.
- **Abnahme:** Tests der Kennungen (gültig, `..`, Großbuchstaben, zu lang, fehlende Teile), der
  Demo-Quelle und der serde-Rundreise. fmt, clippy und test grün, auch mit `TORRO_LANG=de`.

### CH1.1 Gerüst: Requests, Katalog, Rechte
- [ ] erledigt
- **Modell:** Opus · **Braucht:** CH1.0 · **Dateien:** `crates/torro-ipc/src/{lib,api,inbox}.rs`, neu
  `crates/torro-ipc/src/chat.rs` (Antworttypen), `crates/torro-ipc/Cargo.toml`,
  `crates/torro-ipc/src/assistant.rs` (`MyAccess` zeigt die Rechte),
  `crates/torroworkplace/src/{core,mcp}.rs` und `crates/torroworkplace/src/ui/launch.rs` (Stellen, die
  `Rights { … }` bauen, und deren Tests; in `mcp.rs` dazu `annotations()`, `IDEMPOTENT` und
  `INSTRUCTIONS`), `crates/torroworkplace/src/clients.rs` (`Rights { … }` und das Leihen ohne `chat`),
  `crates/torroworkplace/src/ui/agents.rs` und
  `crates/torroassistant/src/chat.rs` (nur die `Area`-Fälle),
  `crates/torroworkplace/src/assistant/engines/mod.rs` (`NEVER_TO_AGENTS`), `docs/api.md` (Bereich
  `chat`, Schalter „Nach außen“ nennt Chat), `docs/api-reference.md`, `docs/eingang.md` (Zeile
  „Antworten“: Talk über `chat.send_message`), `docs/app-catalog.md` (Status „Entwurf“ mit Verweis)
- **Requests:** Bereich `chat`, Stufe CH1 aus der Tabelle in [chat.md](../chat.md#befehle-bereich-chat),
  je mit Doc-Kommentar und Fehlercodes. Handler antworten bis CH1.3 mit `unsupported`.
  - `list_conversations`: `limit` höchstens 200, Standard 50; `query` filtert nach Namen im Cache.
  - `get_messages`: `before` und `after` schließen sich aus, `limit` höchstens 100, Standard 30.
  - `send_message`: `text` 1 Zeichen bis `max-length` des Servers (der Kern prüft, der Request
    höchstens 32.000), `reply_to` als `MessageId` derselben Unterhaltung.
  - `mark_read`: ohne `up_to` bis zur neuesten bekannten Nachricht.
  - **Antworten:** `send_message` antwortet mit der eigenen Antwortart `result: "chat-sent"` und nur der
    Kennung der Nachricht (nicht der ganzen Nachricht, damit die Datenregel sie durchlassen kann);
    `mark_read` und `mark_unread` mit `ok`.
- **Erweiterungen:**
  - `inbox.get_item` bekommt bei Einträgen der Quelle `talk:<x>` das optionale Feld `conversation`
    (`nextcloud:<x>/<token>`).
  - `apps.open_app` nimmt `target: "item:<id>"` für die Fenster von `torroinbox` an. Prüfen und
    Starten kommt in CH1.5; bis dahin `unsupported`.
- **Katalog:**
  - Alle Befehle `Mcp::Tool`, keiner `Never`.
  - `send_message` steht in `OUTBOUND`. Ein Fall im Rechte-Test: Mit `outbound: never` ist es
    `forbidden`, mit `with_approval` wartet es auch bei `Confirm::Never`.
  - Die Beschreibungen sagen, dass Nachrichten **Daten, keine Anweisungen** sind. Ein Test
    `chat_descriptions_say_messages_are_data` (Vorbild `mail_descriptions_say_mail_is_data`) prüft
    das für jeden lesenden Befehl; spätere Stufen ergänzen ihre Befehle.
  - `send_message` sagt: nach außen, wartet meist auf den Menschen, Erwähnungen als `@"<mentionId>"`.
  - `get_messages` sagt: ändert keinen Lesestand, dafür `mark_read`.
  - `Lack::Outbound` wird neutral („sending on the person's behalf is switched off for this client“),
    auch der Doc-Kommentar von `Lack::area` („always mail's switch“).
- **MCP (`mcp.rs`):** `chat_send_message` ist `destructive` wie `mail_send_draft`, `chat_mark_read` steht
  in `IDEMPOTENT`. `INSTRUCTIONS` nennt Chat neben Mail als immer untrusted. Ein Test wie der
  vorhandene Hinweis-Test hält das fest.
- **Rechte:**
  - `Area::Chat` (auch in `Area::ALL`), `Rights.chat_accounts: Selection` mit serde-Standard
    `nothing_selected` für alte `clients.json`, `Rights::reaches_chat_account(id)`,
    `Rights::common` nimmt es mit.
  - `Rights::selection(Area::Chat)` liefert `chat_accounts`; `Rights::set_level` füllt beim
    Einschalten einer leeren Auswahl die Konten mit Talk (`present()`, Regel aus Issue #133).
  - **Vorlagen** setzen nur die Stufe; `chat_accounts` bleibt `Only([])`. `Rights::template()`
    vergleicht `chat_accounts` nicht mit.
  - `Rights::assistant()` bekommt `chat` nicht. `Area::Chat` kommt in `NEVER_TO_AGENTS`, mit Test.
  - **Leihen ohne `chat`** (`clients.rs`, Schlüssel für einen Coding-Agenten): Der Kern streicht dann
    auch alle Quellen `talk:*` aus `inbox_sources`. Sonst läsen Coding-Agenten Titel und letzte Zeile
    von Talk über den Eingang.
- **Abnahme:**
  - Katalog-Tabellentest und Namensregeln grün; alte `clients.json` lädt und sieht kein Konto.
  - Ein neuer Client aus jeder Vorlage sieht kein Chat-Konto, und die Vorlage wird weiter erkannt.
  - Ein Coding-Agent als Antrieb des Assistenten bekommt `chat` nie, auch wenn die Rechte es geben,
    und sieht keine Talk-Einträge im Eingang (Test mit `inbox_sources: All`).
  - Einschalten der Stufe in `chat` mit leerer Auswahl wählt die Konten mit Talk.
  - API-Referenz neu erzeugt (`TORRO_BLESS=1 cargo test -p torro-ipc`); `torro-api-review` ohne Befund.

### CH1.2 Talk im Anbieter: Nachrichten lesen, warten, senden
- [ ] erledigt
- **Modell:** Opus (Agent `nextcloud-expert`) · **Braucht:** CH1.0 · **Dateien:**
  `crates/torro-nextcloud/src/{lib,talk,capabilities}.rs`, `crates/torro-nextcloud/Cargo.toml`,
  `crates/torro-nextcloud/tests/fixtures/talk_*.json` (neu, anonymisiert)
- `talk.rs` setzt `torro_chat::ChatProvider` um (oder ein dünner Adapter in derselben Datei, wenn der
  Trait das Konto braucht). Wird die Datei zu lang, darf sie in `talk/{mod,rooms,chat}.rs` aufgeteilt
  werden; der Bericht nennt das.
  - **Unterhaltungen:** die vorhandene `read_rooms` um `readOnly`, `isFavorite`, `type` 3 = öffentlich,
    `lobbyState` und `canDeleteConversation` (falls nötig) erweitern.
  - **Nachrichten:** `GET /api/v1/chat/{token}` mit `lookIntoFuture=0`, `limit`,
    `lastKnownMessageId`, `setReadMarker=0`, `noStatusUpdate=1`, mit `chat-keep-notifications`
    `markNotificationsAsRead=0`. `X-Chat-Last-Given` für die nächste Seite, 304 = nichts mehr.
    Platzhalter mit `rich::plain`, Erwähnungen und Dateien aus `messageParameters` als eigene Felder.
    `parent` wird zu `reply_to`. Systemnachrichten `reaction*`, `message_edited`, `message_deleted`
    ändern die betroffene Nachricht und werden selbst nicht geliefert.
  - **Warten:** dasselbe mit `lookIntoFuture=1&timeout=30`. 304 nach dem Timeout ist „nichts Neues“,
    kein Fehler.
  - **Senden:** `POST /api/v1/chat/{token}` mit `message`, `replyTo`, `referenceId` (64 Hex-Zeichen,
    SHA-256 aus Kern-Zufall), `silent` nur mit `silent-send`. 201 liefert die Nachricht; 403 →
    `ReadOnly`, 412 → `Lobby`, 413 → `TooLong`, 429 → `Throttled`.
  - **Gelesen/ungelesen:** `mark_read` gibt es; `DELETE /api/v1/chat/{token}/read` mit `chat-unread`.
  - **Erwähnungen:** `GET /api/v1/chat/{token}/mentions?search=…&limit=…`, `mentionId` (mit
    `federation-v1`) sonst `id`.
  - **Capabilities:** `spreed.features` als Menge, `config.chat.max-length`,
    `config.attachments.{allowed,folder}` lesbar machen.
  - **Prüfen:** ob `GET /room?modifiedSince=…` auf allen unterstützten Versionen nur Geändertes
    liefert (Bericht, Offen-Punkt in [chat.md](../chat.md)).
- **API/MCP:** keine (Anbieter).
- **Abnahme:** Tests ohne Netz (`httpmock`, Fixtures nach Talk-Doku): eine Seite Nachrichten mit
  Erwähnung, Datei, Antwort und Systemnachricht; `X-Chat-Last-Given`; Long Polling mit 304; Senden
  mit `referenceId` und `silent`; 403, 412, 413, 429; ein Server ohne `chat-keep-notifications`
  schickt den Parameter nicht; ungültiger Token wird nie in eine URL gebaut.

### CH1.3 Kern: Cache, Live, Befehle
- [ ] erledigt
- **Modell:** Opus · **Braucht:** CH1.1, CH1.2 · **Dateien:** neu
  `crates/torroworkplace/src/chat/{mod,service,cache,live}.rs`,
  `crates/torroworkplace/src/sources/{nextcloud,talk}.rs` (Liste an den Cache weitergeben),
  `crates/torroworkplace/src/{core,account,approvals,audit,inbox}.rs`,
  `crates/torroworkplace/src/assistant/{data_rule,tools}.rs`, `crates/torroworkplace/Cargo.toml`
- **`cache.rs`:** je Konto die Unterhaltungen, je gelesener Unterhaltung höchstens 200 Nachrichten,
  höchstens 20 Unterhaltungen, nach 30 Minuten ohne Leser verworfen. Nur im Speicher.
- **Liste:** Die Schleife des Nextcloud-Kontos (E5.9) gibt jede Liste an den Cache, eine Abfrage für
  Eingang und Chat. `chat.list_conversations` mit einer Liste älter als 10 s stößt im Hintergrund eine
  neue an und antwortet sofort aus dem Cache.
- **`live.rs`:** `get_messages` mit `after` vom App-Schlüssel merkt die Unterhaltung 90 s vor
  (Clients merken nichts vor, sie bekommen den Cache, älter als 10 s einmal neu geholt, je Client
  höchstens eine solche Abfrage zur Zeit); höchstens drei
  Long-Polling-Anfragen zugleich, die älteste Vormerkung fällt zuerst. Ein Fehler hält die Anfrage
  mit wachsender Pause an (2, 4 … 60 s), `Auth` beendet sie.
- **Befehle:** `list_conversations`, `get_messages`, `send_message`, `mark_read`.
  - Rechte: Konto in `chat_accounts`, sonst `not-found`, geprüft vor jedem Zugriff aufs Netz.
  - `send_message` prüft Länge gegen `max-length`, `reply_to` gegen die Unterhaltung, schickt mit
    `referenceId` und legt die Antwort in den Cache.
  - Talk fehlt → `not-connected`; Fehler des Anbieters auf die Codes aus [chat.md](../chat.md).
- **Freigabe-Fakten** (`approvals.rs`): Name und Art der Unterhaltung aus dem Cache, öffentlich,
  föderiert („Includes people on other servers“), der entschärfte Text (in der Zusammenfassung 1.000
  Zeichen, dann „… ({n} more characters)“; der ganze Text bleibt für „Show all“), Autor der zitierten Nachricht,
  „Notifies everyone in the conversation“ bei `@all`, „Sent silently“. Unbekannt → „unknown
  conversation“. Nimmt eine Rechteänderung zwischen Anfrage und „Allow“ das Konto weg, wird nichts
  gesendet. Bei Entscheidung („Allow“, „Deny“) oder Ablauf leert der Kern den Text eines `chat.*`-Requests
  in der Datei der Freigaben sofort, nicht erst nach `KEPT` (heute ein Tag).
- **Drossel:** je Client-Kennung (nicht je Schlüssel, sonst umgehen geliehene Schlüssel sie)
  höchstens 5 Nach-außen-Befehle in `chat` je Minute und 30 je Stunde, sonst `busy`. Gezählt wird beim
  Ausführen, nicht beim Anfragen.
- **Datenregel** (`data_rule.rs`, `tools.rs`):
  - Positivliste `CHAT_ANSWERS_ALLOWED` wie `MAIL_ANSWERS_ALLOWED`: Jede `chat.*`-Antwort wird für
    Modelle anderswo zurückgehalten, außer `ok`, `chat-sent` und Fehlern.
    Spätere Befehle (`suggest_mentions`, `find_people`, `search_messages`) sind damit von selbst gedeckt.
  - `reads_mail` kennt `chat.`, `may_have_read_untrusted` kennt `Area::Chat`: Der Verlauf nach einem
    `chat.*`-Aufruf geht nicht an Modelle anderswo, Notizen danach werden `local_only`.
- **Eingang:** `inbox.get_item` füllt `conversation` bei Talk-Einträgen.
- **Konto:** Konto entfernen oder Remote Wipe leert den Cache, bricht Long Polling ab und streicht das
  Konto aus allen `chat_accounts`.
- **Protokoll:** Befehl, Konto, Prüfsumme der Unterhaltung.
- **API/MCP:** die vier Befehle aus CH1.1, jetzt mit Handlern. MCP-Tools gibt es seit CH1.1.
- **Abnahme:** Tests mit einem Fake-`ChatProvider`:
  - Client ohne Konto: `not-found` vor jedem Netzaufruf.
  - `get_messages` setzt keinen Lesestand (Fake zählt `mark_read`).
  - Vormerken, drei Long Polls, die vierte Vormerkung verdrängt die älteste.
  - Freigabe zeigt Name aus dem Cache, nicht aus dem Request; `@all` steht in der Freigabe.
  - Drossel: die sechste Nachricht in einer Minute ist `busy`, der App-Schlüssel nie.
  - Konto weg → Cache leer, Auswahlen bereinigt.
  - Remote-Modell bekommt keine Nachrichten, lokales schon; nach `chat.get_messages` geht auch der
    weitere Verlauf des Gesprächs nicht an ein Remote-Modell.
  - Ein Client mit `get_messages { after }` merkt nichts vor; zwei gleichzeitige Abfragen eines Clients
    gehen nur einmal ans Netz.
  - Nach „Allow“, „Deny“ oder Ablauf steht kein Chat-Text mehr in der Datei der Freigaben.
  - `send_message` antwortet `chat-sent` mit Kennung, und diese Antwort erreicht ein Remote-Modell.
  - Eine Talk-Fehlermeldung im OCS-Text erscheint nicht in `message`.
  - `torro-api-review` ohne Befund.

### CH1.4 Notification Hub: Verlauf und Antwortfeld im Paneel
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** CH1.3 · **Dateien:** `crates/torroinbox/src/{app,panel,chat}.rs`
  (`chat.rs` neu; welche Dateien das Paneel heute hat, prüft die Aufgabe zuerst und nennt sie im
  Bericht), `crates/torroinbox/src/demo.rs`
- Skill `torro-design` laden.
- **Oberfläche** (Paneel eines Talk-Eintrags mit `conversation`):
  - Verlauf der letzten 20 Nachrichten (`chat.get_messages`), Trenner „New“ beim Lesestand, „Show
    earlier“ lädt mit `before`. Systemnachrichten klein und grau. Dateien als Zeile mit Name.
  - Neues alle 2 s mit `after`, solange das Paneel offen ist.
  - Antwortfeld: Enter sendet (`chat.send_message`), Shift+Enter bricht um, „Reply“ an einer Nachricht
    zitiert sie. Während des Sendens ist das Feld gesperrt; bei Fehler bleibt der Text stehen und die
    Meldung steht darunter. `pending` zeigt „Waiting for approval“ (nur bei Client-Schlüsseln möglich).
  - Nach dem Senden `inbox.set_item_done`.
  - Ohne Recht auf `chat` (Antwort `not-found` oder `forbidden`): nur die letzte Zeile und „Open in
    Talk“.
- **API:** `inbox.get_item`, `chat.get_messages`, `chat.send_message`, `inbox.set_item_done`.
  **MCP:** dieselben Tools.
- **Abnahme:** Tests der Nachrichten (Senden, Fehler behält den Text, Zitat, ohne Recht kein Feld);
  `torro-design-review` mit `--demo`, hell und dunkel, schmal und breit.

### CH1.5 Benachrichtigung „Reply“ und Ziel `item:`
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** CH1.4 · **Dateien:** `crates/torroworkplace/src/{inbox_notify,apps}.rs`,
  `crates/torroinbox/src/main.rs` (`--item`), `crates/torroinbox/src/app.rs` (Eintrag wählen, Feld
  fokussieren)
- **Benachrichtigung:** Ein Talk-Eintrag bekommt den Knopf „Reply“ neben „Done“ und „Snooze 1 h“
  (`Answer::Reply`). Er schickt `apps.open_app { app: torroinbox, target: "item:<id>" }`.
- **`apps.open_app` mit `item:<id>`:** Der Kern prüft, dass der Aufrufer den Eintrag sieht (sonst
  `not-found`), und startet `torroinbox --item=<id>`. Nur geprüfte Zeichen kommen auf die Befehlszeile,
  höchstens 200 Byte.
- **`torroinbox --item <id>`** öffnet den Notification Hub mit dem Eintrag im Paneel und dem Cursor im
  Antwortfeld (bei anderen Einträgen nur das Paneel).
- **API:** `apps.open_app` (Erweiterung aus CH1.1). **MCP:** `apps_open_app`.
- **Abnahme:** Tests von `request(Answer::Reply, …)`, des Startarguments (gültig, zu lang, fremde
  Zeichen) und `target_command`; Client ohne die Quelle bekommt `not-found`.

### CH1.6 Steuerzentrale: Rechte und Freigaben für Chat
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** CH1.3 · **Dateien:** `crates/torroworkplace/src/ui/{agents,launch,approvals}.rs`,
  die Freigabe-Karte in `crates/torroinbox/src/*` (Datei nennt die Aufgabe),
  `crates/torroworkplace/src/ui/accounts.rs` (Dienst „Talk“), die Seite „What the assistant may use“
  (Datei nennt die Aufgabe nach einem Blick in `ui/`)
- **Oberfläche:**
  - Rechte-Zeile „Chat“ mit Stufe und Kontoauswahl (Konten mit Talk), wie die Mail-Zeile.
  - Beim Verbinden eines Agenten sind die Konten mit Talk vorgewählt, solange `chat` eine Stufe hat.
  - Der Hinweis an „Send on my behalf“ nennt Mail, Antworten auf Einladungen und Chat.
  - „What the assistant may use“: Chat aus ab Werk, mit Hinweis „Only for AI on this computer“.
  - Kontoseite: Talk bei den Diensten, „Talk is not installed on this server.“ wenn es fehlt.
  - Freigaben (`ui/approvals.rs` und die Freigabe-Karte im Notification Hub): ein langer Chat-Text ist
    eingeklappt und lässt sich mit „Show all“ ganz lesen, bevor jemand „Allow“ drückt.
- **API:** `clients.update_client` (vorhanden), `accounts.list_accounts`. **MCP:** keins neu
  (`clients.*` ist `Never`).
- **Abnahme:** Tests der Nachrichten (Auswahl ändert `chat_accounts`); `torro-design-review`.

## CH2 Die App `torrochat`

### CH2.1 Gerüst CH2: Personen, neue Unterhaltung, ungelesen, Ziel `chat:`
- [ ] erledigt
- **Modell:** Opus · **Braucht:** CH1.3 · **Dateien:** `crates/torro-ipc/src/{lib,api,chat}.rs`,
  `crates/torro-chat/src/{provider,source,demo}.rs`, `crates/torro-nextcloud/src/talk.rs` (Raum
  anlegen, Personen suchen), `crates/torroworkplace/src/chat/service.rs`,
  `crates/torroworkplace/src/{core,mcp,apps,approvals}.rs`, `docs/api.md`, `docs/api-reference.md`
- **Requests:** `chat.mark_unread`, `chat.suggest_mentions`, `chat.find_people`,
  `chat.create_conversation` aus der Tabelle in [chat.md](../chat.md#befehle-bereich-chat).
  `create_conversation` steht in `OUTBOUND`.
- **Anbieter:**
  - `find_people`: `GET /ocs/v2.php/core/autocomplete/get?search=…&itemType=call&itemId=new&shareTypes[]=0`,
    höchstens 20, mindestens zwei Zeichen.
  - `create_conversation`: `POST /api/v4/room` mit `roomType=1`, `invite=<user-id>`; 200 (vorhanden)
    und 201 (neu) liefern beide die Unterhaltung.
- **Freigabe:** „Start a direct chat with {name}“, Name aus der Antwort der Personensuche im Cache,
  sonst die Kennung.
- **`apps.open_app` mit `target: "chat:<unterhaltung>"`:** Konto in der Reichweite, Unterhaltung im
  Cache, sonst `not-found`. Startet `torrochat --conversation=<id>`; das Ziel gehört zur App
  `torrochat`.
- **MCP:** je Befehl ein Tool; `apps_open_app` nennt das neue Ziel. `chat_mark_unread` in
  `IDEMPOTENT`. Der Test `chat_descriptions_say_messages_are_data` deckt `suggest_mentions` und
  `find_people`.
- **Abnahme:** Tests mit Fake-Anbieter und `httpmock` (Raum vorhanden/neu, Personensuche); Rechte-Test
  für `create_conversation` (nach außen, zählt in der Drossel); Antworten der neuen Befehle gehen nicht an
  Remote-Modelle; `torro-api-review` ohne Befund.

### CH2.2 App `torrochat` mit Demo
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** CH1.0 · **Dateien:** `crates/torrochat/` (neu: `Cargo.toml`,
  `src/{main,app,sidebar,thread,composer,tile}.rs`), Wurzel-`Cargo.toml` (Mitglied),
  `crates/torro-core/src/app.rs` (`AppId::CHAT`, Fensterklasse `torro-chat`, in `AppId::ALL`),
  `crates/torroworkplace/src/tray/menu.rs` (Eintrag „Chat“ ohne Zahl), Installer-Listen, die
  `AppId::ALL` nicht von selbst abdecken (die Aufgabe sucht sie und nennt sie im Bericht)
- Skill `torro-design` laden. Nur gegen `ChatSource`, mit `DemoSource` (`--demo`).
- **Oberfläche** nach [chat.md](../chat.md#oberfläche): Seitenleiste mit Suche, Filter „All“/„Unread“,
  Favoriten, „Note to self“, ab zwei Konten gruppiert; Verlauf mit Gruppen nach Tag und Autor,
  Zitat, Erwähnungen, Dateien, einfachem Markdown ohne Bilder, Trenner „New messages“, älteres beim
  Scrollen; Eingabe mit `@`-Vorschlägen, Zähler ab 90 % der Höchstlänge, „Send without
  notification“; „New chat…“ mit Personensuche; Tasten `⌘K`/`Ctrl+K`, `Alt+↑`/`Alt+↓`, `Esc`; schmal
  Liste oder Verlauf; Mini-Kachel mit den ungelesenen.
- Leere Zustände: kein Konto („Connect Nextcloud“ öffnet die Steuerzentrale), Talk fehlt, keine
  Unterhaltung, offline.
- Links im Text nur `https` und `mailto`, nach einem Klick. „Open“ an Dateien kommt in CH2.3 (Link vom
  Kern).
- **API/MCP:** noch keine (Demo); die Befehle bindet CH2.3 an.
- **Abnahme:** Tests der Nachrichten (Senden, Antworten, Erwähnung einfügen, Filter, Sprung zur
  nächsten ungelesenen); `torro-design-review` mit `--demo` in allen Größenklassen, hell und dunkel.

### CH2.3 `torrochat` an den Kern
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** CH2.1, CH2.2 · **Dateien:** `crates/torrochat/src/*`,
  `crates/torro-ipc/src/chat.rs` (`ChatClient` setzt `ChatSource` über IPC um)
- **Anbindung:** `ChatClient` mit allen Befehlen aus CH1 und CH2. Neues alle 2 s mit `after` für die
  offene Unterhaltung, die Liste alle 10 s.
- **Gelesen:** Fenster im Vordergrund und neueste Nachricht sichtbar → `chat.mark_read`.
- **`--conversation <id>`** öffnet die Unterhaltung (Ziel `chat:` aus CH2.1).
- **Dateien:** „Open“ öffnet den Link `<server>/f/<id>`, den der Kern mitliefert (Feld `link` in der
  Antwort von `get_messages`, gebaut aus dem Ursprung des Kontos und der Datei-Id; ergänzt diese
  Aufgabe in `chat.rs` des Kerns und in `torro-ipc`, verträglich).
- **Notification Hub:** „Open in Chat“ im Paneel, wenn `torrochat` installiert ist (`apps.list_apps`).
- **API:** alle `chat.*` aus CH1 und CH2, `apps.open_app`, `apps.list_apps`. **MCP:** dieselben Tools.
- **Abnahme:** Tests der Startargumente und der Lesestand-Regel (Hintergrundfenster setzt nichts);
  Sichtprüfung gegen einen Nextcloud-Testserver (`docker run nextcloud` mit Talk), im Bericht.

## CH3 Teilen in einen Chat (TL2)

### CH3.1 Anbieter: Datei hochladen und in die Unterhaltung teilen
- [ ] erledigt
- **Modell:** Opus (Agent `nextcloud-expert`) · **Braucht:** CH1.2 · **Dateien:** neu
  `crates/torro-nextcloud/src/files.rs`, `crates/torro-nextcloud/src/{lib,ocs}.rs`,
  `crates/torro-nextcloud/tests/fixtures/files_*`
- **Benutzer-ID:** `GET /ocs/v2.php/cloud/user` → `id`, je Konto gemerkt. Nie aus dem Loginnamen.
- **Hochladen:** `PUT /remote.php/dav/files/<id>/<ordner>/<name>` mit `If-None-Match: *`; bei 412 der
  nächste freie Name „<name> (2).<ext>“ (höchstens zehn Versuche). Ordner aus
  `config.attachments.folder`, fehlt er `/Talk`; fehlt der Ordner auf dem Server, `MKCOL`. Name und
  Ordner ohne `..`, ohne führendes `/` im Namen, Pfadteile prozentkodiert.
- **Teilen:** `POST /ocs/v2.php/apps/files_sharing/api/v1/shares` mit `shareType=10`,
  `shareWith=<token>`, `path`, `referenceId`, `talkMetaData={"caption": …}` (mit `media-caption`,
  sonst ohne und der Kern schickt den Text vorher als Nachricht).
- **Aufräumen:** `DELETE` der hochgeladenen Datei mit `If-Match`, wenn das Teilen scheitert.
- **ChatProvider:** neue Methode `send_file(token, name, mime, data, caption, reference_id)`.
- **API/MCP:** keine (Anbieter).
- **Abnahme:** Tests mit `httpmock`: Upload und Teilen mit Beschriftung; Namenskonflikt → „ (2)“;
  Teilen scheitert → Datei gelöscht; `attachments.allowed: false` → `Unsupported`; Benutzer-ID ≠
  Loginname (Mail als Login).

### CH3.2 `chat.send_message` mit Inhalten
- [ ] erledigt
- **Modell:** Opus · **Braucht:** CH3.1, CH1.3, TL1.1 (`share::card_for`) · **Dateien:**
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torroworkplace/src/chat/{service,share}.rs` (`share.rs`
  neu), `crates/torroworkplace/src/{core,approvals,audit}.rs`, `crates/torroworkplace/src/assistant/*`
  (nur die Zuordnung zu den Bereichen der Inhalte), `crates/torroworkplace/src/mcp.rs` (Beschreibung,
  falls das Schema für `items` Hinweise braucht), `docs/api.md`, `docs/api-reference.md`,
  `docs/teilen.md` und `docs/tasks/teilen.md` (TL2 und Ziel „Talk“ auf `chat.send_message` und „Chat“
  umstellen, TL2 auf diese Aufgaben verweisen)
- **Request:** `chat.send_message` bekommt die optionalen Felder `items: Vec<ShareItem>` und `fields`
  (verträglich). Höchstens 50 Inhalte einer Art, wie `share_card`.
- **Kern (`chat/share.rs`):** Karte über `share::card_for` mit den Rechten des Aufrufers; fehlt das
  Recht auf einen Inhalt, `not-found` für den ganzen Befehl. Geprüft vor der Freigabe und noch einmal
  bei „Allow“ mit derselben Reichweite samt Grenzen eines geliehenen Schlüssels (Vorbild
  `respond_to_invitation`). Dann `send_file` mit dem Text als Beschriftung (leer: Text der Karte).
  Karte ohne Datei → nur Text.
- **Freigabe:** Unterhaltung, Text, jede Karte wie bei Mail („Contact card: Anna Muster (6 fields)“,
  „+ private address“) und „Uploads contacts.vcf to your Talk folder in Nextcloud“.
- **Assistent:** Der Befehl mit `items` gehört auch zu den Bereichen der Inhalte (`contacts`), damit ein
  im Gespräch abgeschaltetes Adressbuch greift.
- **Protokoll:** zusätzlich Art und Zahl der Inhalte, Kennungen als Prüfsumme.
- **MCP:** `chat_send_message` nennt `items` in der Beschreibung.
- **Abnahme:**
  - Zwei Kontakte gehen als eine `contacts.vcf` mit genau den gewählten Feldern und Beschriftung.
  - Ein Agent mit Chat-, aber ohne Kontakt-Recht bekommt `not-found`, und nichts wird hochgeladen.
  - Nimmt eine Rechteänderung zwischen Anfrage und „Allow“ das Adressbuch weg, wird nichts gesendet.
  - Die Freigabe nennt Karten, zusätzliche private Felder und den Upload.
  - `torro-api-review` ohne Befund.

### CH3.3 Ziel „Chat“ im Teilen-Blatt
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** CH3.2, TL1.2, TL1.5 · **Dateien:** `crates/torro-gui/src/share.rs`,
  `crates/torrocontacts/src/*` (nur das Ziel melden und senden)
- Skill `torro-design` laden.
- **Oberfläche:**
  - Ziel „Chat“ im Teilen-Blatt, wenn die App es meldet: ein Konto mit Talk und Recht auf `chat`
    (Antwort von `chat.list_conversations` ohne Fehler).
  - Wahl der Unterhaltung: die letzten zehn mit Suche, dazu ein Feld „Message“ (vorbelegt leer; leer
    heißt: Text der Karte).
  - „Send“ schickt `chat.send_message` mit `items` und `fields`, danach „Sent to {conversation}“ oder
    „Waiting for approval“.
  - Demo: „Sent (demo)“.
- **API:** `chat.list_conversations`, `chat.send_message`. **MCP:** dieselben Tools.
- **Abnahme:** Tests der Nachrichten (Auswahl und Felder gehen mit, ohne Talk kein Ziel);
  `torro-design-review` mit `torrocontacts --demo --open share`.

## CH4 Pflege und Suche

### CH4.1 Anbieter: bearbeiten, löschen, Reaktionen, Stufe, Favorit, Suche
- [ ] erledigt
- **Modell:** Opus (Agent `nextcloud-expert`) · **Braucht:** CH1.2 · **Dateien:**
  `crates/torro-nextcloud/src/talk.rs` (bzw. `talk/`), `crates/torro-chat/src/provider.rs`,
  `crates/torro-nextcloud/tests/fixtures/talk_*`
- `PUT`/`DELETE /api/v1/chat/{token}/{id}` (`edit-messages`, `delete-messages`; 400 → `TooOld`, 405 →
  `Unsupported`), `POST`/`DELETE /api/v1/reaction/{token}/{id}` mit `reaction` (`reactions`),
  `POST /api/v4/room/{token}/notify` mit `level`, `POST`/`DELETE /api/v4/room/{token}/favorite`,
  Unified Search mit dem Anbieter für Talk-Nachrichten (`/ocs/v2.php/search/providers/talk-message/search`;
  Name und Felder gegen die Doku prüfen).
- **API/MCP:** keine (Anbieter).
- **Abnahme:** Tests mit `httpmock` je Endpunkt, auch zu alt (400), Funktion fehlt, fremde Nachricht.

### CH4.2 Gerüst und Kern CH4
- [ ] erledigt
- **Modell:** Opus · **Braucht:** CH4.1, CH1.3 · **Dateien:** `crates/torro-ipc/src/{lib,api,chat}.rs`,
  `crates/torro-chat/src/source.rs`, `crates/torroworkplace/src/chat/service.rs`,
  `crates/torroworkplace/src/{core,mcp,approvals}.rs`, `crates/torroworkplace/src/assistant/data_rule.rs`,
  `docs/api.md`, `docs/api-reference.md`
- **Requests:** Stufe CH4 aus der Tabelle in [chat.md](../chat.md#befehle-bereich-chat).
  `edit_message` ist `Delete`, `delete_message` `Delete`, `react` `Write`; alle drei in `OUTBOUND`.
- **Kern:** nur eigene Nachrichten bearbeiten und löschen (Autor aus dem Cache, sonst `unsupported`);
  Drossel aus CH1.3 zählt Bearbeiten und Reaktionen mit; `set_notification_level` stößt eine neue
  Runde des Eingangs an, damit `rules::talk_place` sofort greift.
- **Freigabe:** Bearbeiten „alt → neu“, Löschen der Anfang der Nachricht, Reaktion mit Emoji und
  Nachricht.
- **Datenregel:** `search_messages` wird für Modelle anderswo zurückgehalten.
- **MCP:** je Befehl ein Tool. `chat_edit_message` und `chat_delete_message` sind `destructive`,
  `chat_set_favorite` und `chat_set_notification_level` in `IDEMPOTENT`. Der Test
  `chat_descriptions_say_messages_are_data` deckt `search_messages`.
- **Abnahme:** Rechte-Tests (Bearbeiten wartet auch mit `outbound: free` bei `Confirm::OnDelete`),
  fremde Nachricht → `unsupported`, Drossel; `torro-api-review` ohne Befund.

### CH4.3 `torrochat`: bearbeiten, löschen, Reaktionen, Stummschalten, Suche
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** CH4.2, CH2.3 · **Dateien:** `crates/torrochat/src/*`,
  `crates/torro-chat/src/demo.rs`
- **Oberfläche:**
  - Kontextmenü eigener Nachrichten: „Edit“ (Pfeil hoch bearbeitet die letzte eigene), „Delete“ mit
    Rückfrage. Nach 24 h bzw. 6 h ausgegraut mit Hinweis.
  - Reaktionen: kleine Leiste mit sechs Emoji beim Überfahren, Zähler unter der Nachricht, Klick auf
    die eigene nimmt sie zurück.
  - Kopf der Unterhaltung: Stern (Favorit), Glocke mit den vier Stufen.
  - Suche in Nachrichten: Feld oben in der Seitenleiste mit Umschalter „Messages“, Treffer springen in
    den Verlauf.
- **API:** die CH4-Befehle. **MCP:** dieselben Tools.
- **Abnahme:** Tests der Nachrichten; `torro-design-review` mit `--demo`.
