# Präsenz: wer gerade da ist (Kontaktliste mit Status)

Stand: 2026-10-15. **Idee mit Recherche, noch nichts entschieden, keine Aufgaben.** Beantwortet die
Frage: **Wer ist gerade da, und wie erreiche ich ihn sofort?**

## Ziel

Ein kleines Fenster wie die Kontaktliste von ICQ: Alle Menschen der eigenen Nextcloud stehen darin,
mit Avatar, Status (online, abwesend, beschäftigt, nicht stören, offline) und Statusnachricht
(„🏖 Urlaub bis Freitag“). Wer online ist, steht oben. Ein Doppelklick öffnet den Chat mit der
Person in Talk. Den eigenen Status setzt man im selben Fenster oder im Tray-Menü, und der Dienst
meldet von selbst „abwesend“, wenn man eine Weile nicht am Rechner ist.

Wie jede Funktion gibt es das dreimal: in der Oberfläche, über die API und über MCP (siehe unten).

## Wer wen sieht

Nutzer derselben Nextcloud sehen sich ab Werk gegenseitig. Dafür braucht es weder Gruppen noch
Teams. Nextcloud bietet zwei Wege:

1. **Sharee-Suche** (`/ocs/v2.php/apps/files_sharing/api/v1/sharees`): die Suche im Teilen-Dialog.
   Ihre Treffer bringen den Status gleich mit (`status`, `message`, `icon`). Die Fotos-Planung nutzt
   sie schon für `photos.find_recipients` ([fotos.md](fotos.md)).
2. **Systemadressbuch** (CardDAV, `z-server-generated--system`): alle Nutzer der Instanz. Die
   Kontakte-Planung führt es nur lesend und ab Werk ausgeblendet ([kontakte.md](kontakte.md)).

Was man sieht, stellt der Admin unter **Verwaltung → Teilen** ein:
- Benutzernamen im Teilen-Dialog automatisch vervollständigen (gibt auch das Systemadressbuch frei).
  Ist das aus, findet man andere nur über den vollen Namen oder die Mail-Adresse.
- Auf Nutzer derselben Gruppe beschränken, auf Treffer im eigenen Telefonbuch beschränken.
- Teilen nur innerhalb der eigenen Gruppen erlauben.

Dazu kommt die Sichtbarkeit je Feld, die jeder Nutzer unter **Persönliche Informationen** einstellt
(Privat, Lokal, Föderiert, Öffentlich). Eine vollständige Nutzerliste über die Provisioning-API
(`/ocs/v1.php/cloud/users`) bekommen nur Admins und Gruppen-Admins; normale Nutzer kommen nur über
die beiden Wege oben an die anderen.

**Gruppen und Teams** regeln etwas anderes:

| | Gruppen | Teams (früher „Circles“) |
|---|---|---|
| Wer legt an | nur der Admin | jeder Nutzer |
| Mitglieder | Nutzer | Nutzer, Gruppen, Mail-Adressen, andere Teams |
| Wozu | Rechte, Verwaltung, Sichtbarkeit beschränken | mit einer festen Runde zusammenarbeiten |
| Freigabetyp | 1 | 7 |

Für die Kontaktliste sind Teams nur als Einteilung interessant („mein Projektteam oben“). Die
bisherigen Pläne (Fotos teilen) kennen nur Personen, Gruppen und Links; Typ 7 fehlt dort noch.

## Was Nextcloud für den Status bietet

Die App **User status** gehört zum Kern von Nextcloud und ist ab Werk an. Geprüft im Quellcode von
Nextcloud (`apps/user_status`, Zweige `stable28`, `stable31`, `stable34`, `stable35` und `master`, Stand 2026-10-15):

**Status** (`OCP\UserStatus\IUserStatus`): `online`, `away`, `busy`, `dnd`, `invisible`, `offline`.
`busy` gibt es mindestens seit Nextcloud 28. Wer `invisible` gesetzt hat, erscheint bei allen
anderen als `offline`. Dazu vordefinierte Nachrichten, die Nextcloud selbst setzt: im Gespräch
(`call`), in einer Besprechung laut Kalender (`meeting`, `busy-tentative`), Abwesenheit
(`out-of-office`, `vacationing`). Eigene Nachrichten haben höchstens 80 Zeichen, ein Emoji und eine
Ablaufzeit (`clearAt`).

**Befehle** (OCS, Präfix `/ocs/v2.php/apps/user_status/api/v1`):

| Befehl | Was |
|---|---|
| `GET /statuses?limit&offset` | Status aller Nutzer, seitenweise; neuere Versionen auch mit `lastId` (Cursor) |
| `GET /statuses/{userId}` | Status einer Person; `404`, wenn sie keinen hat |
| `GET /user_status` | der eigene Status |
| `PUT /user_status/status` | eigenen Status setzen (`statusType`) |
| `PUT /user_status/message/custom` · `/message/predefined` | eigene Nachricht setzen |
| `DELETE /user_status/message` | Nachricht löschen |
| `PUT /heartbeat` (`status=online\|away`) | „ich bin da“ |

**Online ist man nur mit Heartbeat.** `online` setzt kein Mensch von Hand, das macht ein Programm,
das regelmäßig `PUT /heartbeat` schickt (die Weboberfläche, solange sie offen ist). Kommt
15 Minuten lang keiner, fällt der Status zurück auf offline (`INVALIDATE_STATUS_THRESHOLD`). Der
Server schreibt einen neuen Heartbeat erst, wenn der letzte älter als 7 Minuten ist
(`REFRESH_STATUS_THRESHOLD`); alle 5 Minuten zu senden reicht also. Von Hand gesetzte Status
(`away`, `busy`, `dnd`, `invisible`) bleiben, auch ohne Heartbeat.

**Kein Push.** Die User-status-API meldet Änderungen nicht von selbst, und die Ereignisse von
`notify_push` (Dateien, Aktivität, Benachrichtigungen) decken den Status nicht ab. Wir fragen ab,
wie bei den Benachrichtigungen jedes Nextcloud-Konto im Minutentakt.

**Die Liste enthält nicht alle.** `/statuses` liest die Tabelle der App, in der nur Nutzer stehen,
die schon einmal einen Status hatten (Heartbeat oder von Hand). Wer fehlt, ist offline. Für eine
vollständige Liste kommt das Systemadressbuch dazu.

**Sichtbarkeit beim Status (wichtig):** `/statuses` hält sich an die Einstellungen beim Teilen, aber
grob: Ist die automatische Vervollständigung aus oder auf dieselbe Gruppe oder das Telefonbuch
beschränkt, liefert es eine **leere Liste** (so in allen geprüften Zweigen). Nur wenn alle alle
sehen dürfen, kommt die ganze Liste. Ab **Nextcloud 35** können Apps die Liste zusätzlich filtern
(`UserEnumerationFilterEvent`, eingeführt im Mai 2026). `/statuses/{userId}` filtert in keiner
Version: Wer die Kennung kennt, bekommt den Status. Auch die Treffer der Sharee-Suche bringen ihn mit.
Auf Servern mit Beschränkung kommt der Status also nur einzeln, für die Menschen, die der Nutzer
ohnehin sieht.

Quellen: [StatusesController](https://github.com/nextcloud/server/blob/master/apps/user_status/lib/Controller/StatusesController.php),
[HeartbeatController](https://github.com/nextcloud/server/blob/master/apps/user_status/lib/Controller/HeartbeatController.php),
[StatusService](https://github.com/nextcloud/server/blob/master/apps/user_status/lib/Service/StatusService.php),
[IUserStatus](https://github.com/nextcloud/server/blob/master/lib/public/UserStatus/IUserStatus.php),
[UserPlugin der Sharee-Suche](https://github.com/nextcloud/server/blob/master/lib/private/Collaboration/Collaborators/UserPlugin.php).

## Idee für den Aufbau

1. **Wer steht in der Liste:** die Nutzer aus dem Systemadressbuch. Ist es gesperrt, nur Menschen,
   die der Nutzer selbst hinzufügt (gefunden über die Sharee-Suche). Bleibt die Liste leer, sagt sie
   warum („Your Nextcloud does not show other users“). Status zeigen wir nur für Menschen in
   dieser Liste.
2. **Abfragen:** im Takt der vorhandenen Nextcloud-Schleife (60 s), solange ein Fenster mit der
   Liste offen ist, sonst gar nicht. Liefert `/statuses` etwas, reicht ein Aufruf (seitenweise). Ist
   die Liste leer, weil der Admin beschränkt hat, fragen wir einzeln (`/statuses/{userId}`), nur für
   die Menschen aus Schritt 1. Bei großen Instanzen braucht das eine Obergrenze (offen).
3. **Heartbeat:** Der Dienst schickt alle 5 Minuten `online` oder `away`, je nach Leerlaufzeit des
   Systems. Ab Werk **aus**, ein Schalter „Show others when I am online“ schaltet ihn ein. Bei
   gesperrtem Bildschirm oder Ruhezustand kein Heartbeat, dann fällt der Status von selbst auf offline.
4. **Eigener Status:** Tray-Menü und Fenster setzen Status und Nachricht. Später: „nicht stören“ an
   einen Fokus-Modus oder eine Workspace-Vorlage koppeln.
5. **Chat:** Doppelklick legt in Talk die Unterhaltung zu zweit an oder öffnet die vorhandene
   (`POST /ocs/v2.php/apps/spreed/api/v4/room`, `roomType=1`, `invite=<userId>`). Talk ist im
   Eingang schon Quelle (`talk:`); die Chat-App ist im [App-Katalog](app-catalog.md) eine Idee.
6. **Mehrere Konten:** je Nextcloud-Konto eine Gruppe in der Liste. Föderierte Server zeigen keinen
   Status; Menschen auf anderen Servern fehlen deshalb.

## Drei Wege (Vorschlag)

Neuer Bereich `presence`, Rechte je Bereich wie üblich. Namen sind Vorschläge.

- **Oberfläche:** Fenster „People“ (eigene kleine App oder Ansicht von `torrocontacts`, offen),
  gruppiert nach online, abwesend, beschäftigt, nicht stören, offline; eigener Status im Tray-Menü.
- **API:**
  - `presence.list_people { account? }` · `Read`: Menschen mit Status und Nachricht.
  - `presence.get_own_status { account }` · `Read`
  - `presence.set_own_status { account, status, message?, icon?, clear_at? }` · `Write` mit
    „Nach außen“: Den Status sehen alle Kollegen, wie eine gesendete Nachricht.
  - Den Heartbeat-Schalter setzt `settings.set_setting` (`Admin`), kein eigener Befehl.
  - Chat öffnen gehört in den Bereich `talk`, wenn es ihn gibt.
- **MCP:** `presence_list_people`, `presence_get_own_status`, `presence_set_own_status`.

**Sicherheit und Datenschutz:**
- Wer wann da ist, ist eine Angabe über andere Menschen. Für Agenten ist `presence` ab Werk aus.
- Statusnachrichten sind Text von anderen Menschen und gehen mit `untrusted: true` an Agenten, wie Mail.
- `set_own_status` wartet für Agenten auf eine Freigabe, solange „Nach außen“ nicht frei ist.
- Ins Protokoll kommen weder Namen noch Statusnachrichten.

## Offen

- An einem echten Server prüfen: Welche Version läuft, stimmt die leere Liste bei eingeschränkter
  Sichtbarkeit, und welche Nutzer zeigt das Systemadressbuch dann?
- Obergrenze für einzelne Abfragen, wenn `/statuses` leer ist (wie viele Menschen, wie oft).
- Eigene App oder Ansicht in `torrocontacts`? Die Kontakte-App kennt das Systemadressbuch schon.
- Heartbeat ab Werk aus oder an? Vorschlag: aus, weil „ich bin online“ etwas über den Menschen verrät.
- Teams als Gruppen in der Liste zeigen (Teams-API der App `circles`)?
- Vor den Aufgaben `torro-api` laden und Requests, Katalog und Rechte im Detail festlegen.
