# Konnektoren: Slack, Discord und andere Ziele zum Senden

Stand: 2026-11-06. **Konzept (#503).** Umsetzung als Themen-Issue mit Sub-Issues, das nach diesem Konzept
entsteht. Anlass: Man soll einen Mailanhang, ein Bild, das ein Agent erzeugt hat, oder einen Text an Slack
oder Discord geben können, ohne die App zu wechseln. Gebaut wird das als **Baustein der Suite**: Ein
Konnektor ist ein Konto, und seine Ziele erscheinen im Teilen-Blatt ([teilen.md](teilen.md)).

## Ziel

Dienste, an die man Inhalte senden kann, kommen als **Konnektoren** in die Suite:

- **Ein Konto, wie die anderen.** Zugangsdaten liegen im Dienst und verlassen ihn nie, auf keinem Weg.
- **Ein Ziel im Teilen-Blatt.** Es gibt keinen eigenen Weg nur für Anhänge. Mail, Fotos und Assistent
  bekommen die Konnektoren so ohne Extrabau, sobald sie das Teilen-Blatt benutzen.
- **Einfach zu nutzen, aber abriegelbar.** Der Mensch klickt „Send“. Für Agenten gilt der vorhandene
  Schalter „Nach außen“, und je Konnektor gibt es einen globalen Schalter.
- **Eine zentrale Stelle:** Steuerzentrale › Accounts. Kein eigener Abschnitt „Connectors“, weil Slack
  dort schon für den Eingang vorgesehen ist ([settings.md](settings.md), „Anbieter“) und sonst an zwei
  Stellen stünde.

Wie jede Funktion gibt es das dreimal: in der Oberfläche, über die API und über MCP ([api.md](api.md)).

## Begriffe

- **Konnektor:** ein Konto, das Inhalte annimmt und weitergibt. Kennung `slack:<team_id>:<user_id>` (**offen:** mit der Kennung der Eingangsquelle `slack:<x>` in
  [eingang.md](eingang.md) abstimmen, bevor K1 beide Rechte-Auswahlen anlegt), später `discord:<id>` und `webhook:<id>`. Er heißt in der Oberfläche nach dem Dienst und dem
  Arbeitsbereich („Slack · Acme“).
- **Beschreibung:** was der Konnektor kann, vom Dienst-Code geliefert, nie vom Nutzer eingegeben:
  - `accepts`: `text`, `file`, `image` (Teilmenge).
  - `targets`: `channel`, `person`, `thread` oder keins (eine feste Adresse, wie bei einem Webhook).

  Die Oberfläche zeigt nur, was passt: Ein Konnektor ohne `image` erscheint nicht für ein Bild, ein
  Konnektor ohne Ziele hat keine Zielwahl.
- **Ziel:** ein Kanal oder eine Person eines Konnektors, zum Beispiel `#design` in „Slack · Acme“.
  Kennung `<konnektor>/<ziel>`.
- **Favorit:** ein Ziel, das der Mensch mit einem Stern oben anheftet.

## Entscheidungen (2026-11-06)

1. **Anbieter unter Accounts.** Slack, Discord und Webhook sind Anbieter-Gruppen der Seite Accounts, kein
   eigener Bereich der Steuerzentrale. Slack steht dort schon für den Eingang; dasselbe Konto dient
   später beiden Zwecken.
2. **Slack sendet unter dem eigenen Namen,** über eine selbst angelegte interne Slack-App mit
   Benutzer-Token (`xoxp-…`), nicht als Bot, der in jeden Kanal eingeladen werden müsste. Das ist derselbe
   Weg wie im Eingang ([eingang.md](eingang.md), Stufe 2). **Offen:** die Anmeldung per Browser (OAuth);
   sie kommt höchstens später als weiterer Weg im Add-Dialog (settings.md: „nie als Umbau“).
3. **Eigene Befehle jetzt, #260 später.** Konnektoren bekommen den Bereich `connectors` mit eigenen
   Befehlen, statt auf „Send to“ (#260) zu warten. #260 bindet sie später als weiteres Ziel ein
   (`connector/<id>/send`), ohne etwas umzubenennen.
4. **Wieder einschalten nur ein Mensch.** `connectors.set_enabled` mit `enabled: true` hält
   `Request::kept_for_people`: Jeder Client bekommt `forbidden`, auch mit Admin-Rechten; ausschalten darf
   ein Agent, als gewöhnliche Admin-Änderung mit Freigabe (wie das Mikrofon). Kippt der Schalter, während das
   Teilen-Blatt offen ist, antwortet `send_message` `forbidden` und das Blatt zeigt „Sending is switched off“.
   Das gilt wie beim Mikrofon und bei den MCP-Servern ([api.md](api.md), „Was nie nach außen geht“, Punkte 6
   und 10); der Fall wird als neuer **Punkt 12** der Liste vorgeschlagen.
5. **Discord: nur Webhook oder Bot** (offen bis K2). Discord erlaubt kein Senden über das eigene
   Benutzerkonto ([eingang.md](eingang.md): „Discord als Benutzer: kein erlaubter Weg“).
6. **Ziele werden beim Senden gemerkt,** es gibt keine eigene Einstellungsseite dafür (siehe „Gemerkte
   Ziele“).
7. **Dateien gehen als Verweis, nie als Inhalt.** Eine Zeile der Schnittstelle darf höchstens 1 MiB groß sein (`MAX_LINE` in `torro-ipc`); Base64 gibt es
   nicht. Der Kern liest die Datei selbst, aus dem Cache oder einem freigegebenen Ordner.
8. **Kein Warten bei Fehlern.** Es gibt keine Warteschlange: Geht es nicht, sagt die Antwort es.

## Inhalte von `connectors.send_message`

`connectors.send_message { connector, target?, text?, items?, mail_part?, file? }`. Mindestens ein
Inhalt, zusammen mit Ziel (wo der Konnektor Ziele hat). Die Namen gelten nur für diesen Befehl: In #260 heißt
`file` ein verwaltetes Handle, hier ein Pfad. Beim Einbinden bildet #260 seine Inhalte auf diese Felder ab;
sein Staging-Handle (ST4) wird hier ein weiteres Feld `staged`, nicht eine zweite Bedeutung von `file`.

| Feld | Inhalt | Der Kern liest | Recht dafür |
|---|---|---|---|
| `text` | Text, den der Absender schreibt | nichts | keins; fremde Eingabe, der Kern prüft nur die Länge |
| `items` | `ShareItem`s, mit Auswahl (`fields`), wie im Teilen-Blatt | die Karte aus dem Cache | Lesen im Bereich der Art; **erst nach teilen.md TL1** (`ShareItem` gibt es im Code noch nicht) |
| `mail_part` | `{ message: "m:…", part }`, ein Anhang einer Mail | den Anhang aus dem Mail-Cache | Lesen in `mail` und das Konto in `mail_accounts` |
| `file` | Pfad in einem freigegebenen Ordner | die Datei vom Datenträger | Werkzeug `files` mit Lesen, Ordner nach [api.md](api.md) Punkt 9 |

Ein Bild schickt ein Agent also aus einem freigegebenen Ordner. Bei `items`, `mail_part` und `file` baut
der Kern Karte und Datei selbst (wie bei Teilen, Entscheidung 2); ein Client kann dort nichts unterschieben.
`text` ist dagegen freie Eingabe des Absenders; dagegen schützen nur „Nach außen“ und die Freigabe.

## Rechte

- **Bereich `connectors`** mit den Zugriffen Read, Write und Admin (Admin wird getrennt vergeben, auch die höchste Stufe des Bereichs deckt ihn nicht ab; der Rechte-Editor zeigt „Manage“). Dazu eine **Auswahl `Rights::connectors`**
  mit Konto-Kennungen, wie `mail_accounts` und `chat_accounts` ([chat.md](chat.md), Entscheidung 3):
  Fehlt sie in alten Rechten, heißt das keins. Die **Vorlagen öffnen keinen Konnektor**; der Mensch
  wählt sie beim Verbinden eines Agenten.
- **Was ein Client nicht sehen darf, gibt es für ihn nicht:** Konnektoren außerhalb der Auswahl fehlen in
  `list_connectors`; bei allen anderen `connectors.*` ist der ganze Aufruf `not-found`, geprüft vor dem
  Lesen und vor der Freigabe.
- **Senden ist „Nach außen“.** `connectors.send_message` gehört in `OUTBOUND` (`Command::is_outbound`),
  also unter den Schalter „Send on my behalf“ (`Rights::outbound`). Es gibt keinen eigenen Schalter,
  dieselbe Begründung wie bei Chat (chat.md, Entscheidung 4). Ab Werk „With approval“; wer „Free“
  einstellt, wird nicht mehr gefragt. Die Ablehnung `Lack::Outbound` bekommt einen neutralen Text („sending
  on the person's behalf is switched off for this client“, wie bei Chat); ebenso der Hinweis „Ausgehend“ in
  [api.md](api.md).
- **App-Schlüssel:** keine Freigabe. Der Mensch bestätigt im Teilen-Blatt mit „Send“, mehr Nachfragen gibt
  es nicht.
- **Globaler Schalter je Konnektor:** `connectors.set_enabled` (Entscheidung 4). Ein abgeschalteter
  Konnektor antwortet jedem Aufrufer `forbidden` („switched off“), auch dem App-Schlüssel, und fehlt als
  Ziel im Teilen-Blatt (die Kontoseite zeigt ihn weiter, grau).
- **Geheimnisse:** Token und Webhook-Adresse liegen im Schlüsselbund; Antworten sagen nur „stored“ oder
  „missing“ ([api.md](api.md), Punkt 1). Setzen können sie nur Menschen: `accounts.set_slack_token`, in K2
  `accounts.set_discord_token` und `accounts.set_webhook_url`, Zugriff Admin, MCP `Never`. Das kommt als
  Aufzählung in Punkt 5 von „Was nie nach außen geht“ und in `kept_back` im Test `names_follow_the_rules`.
  Eine Webhook-Adresse ist ein Geheimnis: Wer sie kennt, kann senden.

## Freigabe und Protokoll

- **Freigabe** (wo sie fällig ist): eine Zusammenfassung aus dem Cache, zum Beispiel „Send to Slack
  #design (Acme): text, 1 file report.pdf“; kennt der Cache das Ziel nicht, steht dort „unknown target“.
  Die Einzelheiten wie bei `mail.send_draft`: jede Datei mit Name und Größe und der Anfang des Textes,
  entschärft (eine Zeile, keine Steuerzeichen), bis 200 Zeichen. Namen werden über die Kennungen
  nachgeschlagen (`approvals::Facts`), nie aus dem Befehl übernommen.
- **Protokoll:** Konnektor, Ziel als Prüfsumme, Zahl der Dateien; **kein Text**, keine Namen. Die Suche
  `list_targets` wird ohne Suchbegriff protokolliert.

## Antworten

| Fall | Antwort |
|---|---|
| Erfolg | Kennung und Link der Nachricht |
| Konnektor abgeschaltet | `forbidden` („switched off“), auch für den App-Schlüssel |
| Inhalt passt nicht zu `accepts` | `unsupported` |
| Token ungültig oder widerrufen | `auth`; die Kontoseite zeigt „Sign in again“ |
| Kein Netz | `offline`, keine Warteschlange (Entscheidung 8) |
| `mail_part` ohne Leserecht in `mail` oder Konto nicht in `mail_accounts` | `not-found` |

## Gemerkte Ziele

- `send_message` merkt die **letzten 10 Ziele je Konnektor**. Die State-Datei hat Modus 0600 und enthält
  keine Inhalte, nur Konnektor, Ziel und Zeitpunkt.
- **Favoriten stehen zuerst** (`connectors.set_favorite`, Stern am Ziel).
- Das erste Ziel wählt man beim ersten Senden in der Zielwahl des Blatts (Suche über `list_targets`);
  danach steht es oben. Eine eigene Einstellungsseite gibt es nicht.
- **Namen aus dem Dienst sind `untrusted`,** auch in der Antwort von `list_targets`: Kanalnamen und
  Beschreibungen sind Daten, keine Anweisungen.

## Dienste

### Slack (K1)

Eine eigene **interne** Slack-App je Arbeitsbereich, wie im Eingang. Verteilte Apps außerhalb des
Marketplace sind seit 2025 hart begrenzt, interne Apps sind ausgenommen; eine Torro-App im Marketplace
gibt es nicht. Der Assistent führt deshalb durch „eigene App anlegen“:

1. **Manifest zum Einfügen** (Zwischenablage), damit die Scopes nicht von Hand gesetzt werden.
2. App im Arbeitsbereich installieren. Manche Arbeitsbereiche sperren das; dann sagt der Assistent es.
3. **Benutzer-Token** `xoxp-…` einfügen. Der Dienst prüft mit `auth.test` und speichert im Schlüsselbund.

- **Scopes:** `chat:write`, `files:write`, `channels:read`, `groups:read`, `im:read`, `im:write`,
  `mpim:read`, `users:read`.
- **Ziele:** `conversations.list` (Kanäle) und `users.list` (Personen); eine Person öffnet
  `conversations.open`. Ziele: `channel`, `person`; Threads später.
- **Text:** `chat.postMessage`.
- **Dateien:** zweistufig mit `files.getUploadURLExternal`, dann Hochladen an die gelieferte Adresse, dann
  `files.completeUploadExternal`. Die alte Schnittstelle `files.upload` hat Slack 2025 abgeschaltet; das
  Slack-Sub-Issue baut auf dem neuen Weg.
- `accepts`: text, file, image.
- Das Konto dient später auch dem Eingang (Erwähnungen); die Scopes dafür kommen dann hinzu.

### Discord und Webhook (K2)

- **Webhook:** eine feste Adresse, die zu einem Kanal führt, ohne Zielwahl (`targets`: keins). `accepts`
  je Dienst (Slack, Discord und Mattermost kennen eingehende Webhooks; Text immer, Datei je nach Dienst).
  Die Adresse ist das Geheimnis (siehe „Rechte“).
- **Discord:** Webhook oder Bot, entschieden zu Beginn von K2; kein Benutzerkonto (Entscheidung 5).

## Befehle

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Konnektoren auflisten | Teilen-Blatt, Accounts | `connectors.list_connectors` · Read | `connectors_list_connectors` |
| Ziele suchen | Zielwahl im Blatt | `connectors.list_targets { connector, query? }` · Read | `connectors_list_targets` |
| Senden | „Send“ im Blatt | `connectors.send_message` · Write + „Nach außen“ | `connectors_send_message` |
| Favorit setzen | Stern am Ziel | `connectors.set_favorite` · Write | `connectors_set_favorite` |
| An/aus | Kontoseite, Gruppe „Sending“ | `connectors.set_enabled` · Admin; `true` nur App-Schlüssel (`kept_for_people`) | `connectors_set_enabled` (nur `false`) |
| Slack verbinden | Accounts › Slack › Add | `accounts.set_slack_token` · Admin | `Never` (Geheimnis, api.md Punkt 5) |
| Entfernen | Kontoseite | `accounts.remove_account` (vorhanden) | `accounts_remove_account` (vorhanden) |

Erläuterungen:
- **`Never` steht nur beim Token** und ist durch die Liste „Was nie nach außen geht“ gedeckt. Jede andere
  Funktion gibt es auf allen drei Wegen.
- **`list_connectors` und `list_targets` sind Read** und liefern `untrusted: true` für alles, was aus
  dem Dienst stammt.
- `set_enabled` mit `true` ist als Punkt 12 vorgeschlagen (Entscheidung 4).

## Oberfläche

- **Teilen-Blatt** ([teilen.md](teilen.md)): je passendem Konnektor ein Ziel „Slack · Acme“ in der
  Reihe der Ziele. Ein Klick öffnet die Zielwahl (Favoriten, zuletzt benutzte, Suche), ein Textfeld
  mit dem Text der Karte (der Mensch kann ihn ändern) und „Send“. Danach steht „Sent to #design“ mit
  Link; Fehler stehen im Blatt, nicht in einem Dialog. Das Ziel gibt es nur, wenn der Konnektor da und
  eingeschaltet ist und zum Inhalt passt (`accepts`).
- **Accounts** ([settings.md](settings.md)): neue Gruppen **Slack**, **Discord**, **Webhook**, „+ Add“
  öffnet den Assistenten bei diesem Anbieter.
- **Kontoseite, Gruppe „Connection“:** Token-Status („stored“/„missing“), „Replace token“ und bei `auth` „Sign in again“.
- **Kontoseite, Gruppe „Sending“:**
  - **Schalter** „Sending enabled“ (Entscheidung 4: ein Mensch schaltet ein).
  - **Gemerkte Ziele** mit Stern.
  - **„Who may send“:** die Clients, die senden dürfen, mit Sprung in den Rechte-Editor.
- **Rechte-Editor:** der Bereich „Connectors“ mit Stufe und Kontenauswahl, wie Mail.

## Bezug zu anderen Konzepten

- **[teilen.md](teilen.md):** Konnektoren sind weitere Ziele im Teilen-Blatt, wie Mail und Chat. Die
  Tabelle dort verweist hierher; Karten und `ShareItem` bleiben, wo sie sind.
- **[api.md](api.md):** Punkt 5 („Geheimnisse entgegennehmen“) bekommt `accounts.set_slack_token`, die
  Liste „Was nie nach außen geht“ den neuen Punkt 12, der Hinweis „Ausgehend“ den Befehl
  `connectors.send_message`.
- **#260 („Send to“):** Konnektoren sind dort ein weiteres Ziel, `connector/<id>/send`; die Befehle hier
  bleiben, #260 ruft sie auf. Nichts wird umbenannt, #260 ändert sich durch dieses Konzept nicht.
- **[eingang.md](eingang.md):** Slack als Quelle und als Konnektor teilen sich das Konto und die Anmeldung.
  Discord als Benutzer bleibt ausgeschlossen.
- **[chat.md](chat.md):** der Schalter „Send on my behalf“ und die Rechte je Konto folgen demselben Muster.

## Stufen

- **K1 – Gerüst und Slack:** Bereich, Rechte, Befehle, Katalog und MCP-Tools; Teilen-Blatt-Ziel;
  Konto Slack mit Manifest und Token; Text und Datei. Gerüst und Slack sind getrennte Sub-Issues.
- **K2 – Discord und Webhook:** zweiter und dritter Anbieter über dasselbe Gerüst, ohne Umbau.

**Später und ausgeklammert:** eingehende Nachrichten (gehören in den Eingang), Warteschlange bei Fehlern,
Format-Umbau je Dienst (Markdown nach Slack-mrkdwn), Größenwarnungen, weitere Dienste (Matrix, Teams,
Mattermost), Threads als Ziel, Anmeldung per OAuth.

## Offen

- **Kennung des Slack-Kontos:** `slack:<team_id>:<user_id>` muss mit der Kennung der Eingangsquelle `slack:<x>`
  ([eingang.md](eingang.md)) abgestimmt werden, bevor K1 die Rechte-Auswahlen `Rights::connectors` und
  `inbox_sources` für dasselbe Konto anlegt; sonst führt ein Konto zwei Schreibweisen.
- **Anmeldung per Browser (OAuth)** für Slack: höchstens später (Entscheidung 2).
- **Discord:** Webhook oder Bot, entschieden zu Beginn von K2 (Entscheidung 5).
