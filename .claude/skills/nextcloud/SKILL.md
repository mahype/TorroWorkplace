---
name: nextcloud
description: Wissen über Nextcloud als Server für TorroWorkplace – Anmeldung (Login Flow v2, App-Passwörter, Widerruf, Remote Wipe), OCS-API, WebDAV/CalDAV/CardDAV, Sync, Capabilities und die APIs der Zusatz-Apps (Tasks, Deck, Notes, Talk, Notifications, Files, Contacts …). Laden, wenn Code mit Nextcloud spricht oder eine neue App an Nextcloud angebunden werden soll.
---

# Nextcloud für Torro

Nextcloud ist der erste und wichtigste Server der Suite. Der **Kern** (`torroworkplace`) hält das
Konto und spricht mit Nextcloud; die Apps sprechen nur mit dem Kern (`torro_pim::PimSource`,
IPC). Neue Nextcloud-Funktionen kommen also in den Kern bzw. in einen Provider-Crate, nie direkt
in eine App.

## Was es im Code schon gibt

| Wo | Was |
|---|---|
| `crates/torro-pim-caldav/src/nextcloud.rs` | `normalize_server`, Login Flow v2 (`start_login`, `LoginFlow::poll`), `dav_url` |
| `crates/torro-pim-caldav/src/lib.rs` | `http_client()` – User-Agent `Torro Workplace (<host>)` = Gerätename in Nextcloud |
| `crates/torro-pim-caldav/src/client.rs` | CalDAV: `current-user-principal` → `calendar-home-set` → Sammlungen, Termine, Aufgaben |
| `crates/torro-pim-caldav/src/write.rs` | iCalendar schreiben, PUT/DELETE mit ETag |
| `crates/torroworkplace/src/account.rs` | Konto + App-Passwort im Schlüsselbund, Anmelden, `forget`, `caldav()` |
| `crates/torro-gui/src/connect.rs` | Anmeldebildschirm der Apps |

Vor neuem Code immer diese Dateien lesen und wiederverwenden (z. B. `http_client`, `http_error`).

## Anmeldung und Sitzung

- **Login Flow v2** ist gesetzt (wie die offiziellen Clients): `POST /index.php/login/v2` →
  Login-URL im Browser öffnen → Poll-Endpunkt mit `token` abfragen (404 = noch nicht, 200 =
  `server`, `loginName`, `appPassword`). Token gilt 20 min. Wir sehen nie das Passwort; 2FA und
  SSO laufen im Browser.
- Danach **Basic Auth mit `loginName` + App-Passwort** für alles (DAV und OCS).
- **`loginName` ≠ Benutzer-ID.** Login kann eine E-Mail sein; DAV-Pfade brauchen die ID. Darum die
  ID immer entdecken (`current-user-principal`, oder OCS `GET /ocs/v2.php/cloud/user` → `id`),
  nie aus dem Loginnamen bauen.
- **Abmelden heißt widerrufen:** `DELETE /ocs/v2.php/core/apppassword` (Basic Auth, Header
  `OCS-APIRequest: true`), *dann* lokal löschen. Sonst bleibt ein gültiges Gerät in
  „Geräte & Sitzungen“ stehen. (Stand heute: `account::forget` löscht nur lokal.)
- **Remote Wipe** unterstützen, wie es Nextcloud von Clients erwartet: bekommt der Client 401/403,
  `POST /index.php/core/wipe/check` mit `token=<app-password>` prüfen; bei `{"wipe": true}` lokale
  Daten und Schlüssel löschen und `POST /index.php/core/wipe/success` melden.
- **401 mit gespeichertem App-Passwort** = widerrufen oder Konto gesperrt → Nutzer neu anmelden
  lassen, nicht in Schleife wiederholen. Nextcloud hat Brute-Force-Schutz und drosselt
  (verzögerte Antworten, 429); wiederholte Fehlversuche treffen den Nutzer, nicht uns.
- **503** + `status.php` → `"maintenance": true`: Wartungsmodus, später erneut versuchen, kein Fehlerdialog.
- Passwörter/App-Passwörter nie loggen, nie in URLs, nur im Schlüsselbund (`keyring`).

## Server erkunden

- `GET /status.php` (ohne Anmeldung): `installed`, `maintenance`, `version`, `productname`.
- `GET /ocs/v2.php/cloud/capabilities` (Header `OCS-APIRequest: true`, `Accept: application/json`):
  Version, installierte Apps und ihre Fähigkeiten (`spreed`, `notes`, `deck`, `notify_push`,
  `user_status`, `files_sharing` …). **Funktionen über Capabilities erkennen, nicht über
  Versionsnummern.** Fehlt eine App, die UI sagt es freundlich, statt einen Fehler zu zeigen.
- Nextcloud unterstützt etwa die letzten drei Hauptversionen; ältere nicht extra bedienen.

## OCS-API (Nextcloud-eigene REST-API)

- Basis `/ocs/v2.php/…` (v2 liefert echte HTTP-Statuscodes; v1 antwortet immer 200 und steckt den
  Status in `ocs.meta.statuscode`).
- Pflicht-Header `OCS-APIRequest: true` (sonst CSRF-Fehler), `Accept: application/json`.
- Antwort-Hülle: `{"ocs": {"meta": {...}, "data": ...}}` – einmal generisch entpacken.
- Referenz: jede Server-/App-Version hat eine `openapi.json` (im jeweiligen GitHub-Repo
  `nextcloud/<app>`), Übersicht: https://docs.nextcloud.com/server/latest/developer_manual/client_apis/

## DAV (WebDAV, CalDAV, CardDAV) – Basis `/remote.php/dav/`

| Pfad | Inhalt |
|---|---|
| `calendars/<uid>/` | Kalender **und** Aufgabenlisten (VEVENT/VTODO, siehe `supported-calendar-component-set`) |
| `calendars/<uid>/trashbin/` | gelöschte Termine (Papierkorb des Kalenders) |
| `addressbooks/users/<uid>/` | Adressbücher (CardDAV, vCard) |
| `files/<uid>/` | Dateien (WebDAV; `SEARCH`-Methode für Suche) |
| `uploads/<uid>/` | Chunked Upload für große Dateien |
| `trashbin/<uid>/`, `versions/<uid>/` | Papierkorb und Versionen der Dateien |

- Entdeckung: `/.well-known/caldav` bzw. `carddav` leiten weiter; danach `current-user-principal` →
  `calendar-home-set` / `addressbook-home-set`. Nie Pfade raten.
- **Schreiben:** neu mit `If-None-Match: *`, ändern/löschen mit `If-Match: <etag>`. 412 = jemand
  anderes war schneller → neu laden, Konflikt dem Nutzer zeigen, nichts überschreiben.
- **Sync:** `REPORT sync-collection` (RFC 6578) mit `sync-token` liefert nur Änderungen; Nextcloud
  unterstützt das für Kalender, Adressbücher und Dateien. Fallback `getctag`/`getetag`-Vergleich.
  Abgelaufenes Token (Fehler `valid-sync-token`) → einmal vollständig neu laden.
- **Wiederholungen:** `calendar-query` mit `time-range`; mit `<c:expand>` liefert der Server
  (Sabre) einzelne Vorkommen. Beim Ändern immer das Master-Objekt + `RECURRENCE-ID` beachten.
- Aufgaben: Nextcloud **Tasks** ist nur eine Oberfläche; die Daten sind VTODO über CalDAV
  (`RELATED-TO` für Unteraufgaben, `PERCENT-COMPLETE`, `STATUS`, `PRIORITY`, `DUE`).
- Nextcloud-Eigenheiten: Farbe in `http://apple.com/ns/ical/:calendar-color` (auch `#RRGGBBAA`),
  geteilte Kalender tragen `http://owncloud.org/ns:owner-principal` und können schreibgeschützt sein
  (`current-user-privilege-set` prüfen, bevor die UI „Bearbeiten“ anbietet).

## Zusatz-Apps (Auswahl, jeweils über Capabilities prüfen)

| App | Weg | Einstieg |
|---|---|---|
| Calendar / Tasks | CalDAV | s. o. |
| Contacts | CardDAV | `addressbooks/users/<uid>/` |
| Files | WebDAV + OCS Sharing | `files/<uid>/`, `/ocs/v2.php/apps/files_sharing/api/v1/shares` |
| Notifications | OCS | `/ocs/v2.php/apps/notifications/api/v2/notifications` |
| Push (`notify_push`) | WebSocket | URL aus Capabilities; meldet `notify_file`, `notify_activity`, `notify_notification` – statt Polling |
| User status | OCS | `/ocs/v2.php/apps/user_status/api/v1/user_status` |
| Unified Search | OCS | `/ocs/v2.php/search/providers`, dann `…/providers/<id>/search?term=` |
| Activity | OCS | `/ocs/v2.php/apps/activity/api/v2/activity` |
| Talk (spreed) | OCS | `/ocs/v2.php/apps/spreed/api/v4/room`, Nachrichten `…/api/v1/chat/<token>` |
| Deck | REST | `/index.php/apps/deck/api/v1.0/boards` (+ `OCS-APIRequest: true`) |
| Notes | REST | `/index.php/apps/notes/api/v1/notes` (ETag/`If-None-Match` für Sync) |
| Bookmarks | REST | `/index.php/apps/bookmarks/public/rest/v2/bookmark` |
| Mail | – | keine stabile öffentliche API → direkt IMAP/SMTP |

Pfade und Versionen ändern sich zwischen App-Versionen. **Vor der Umsetzung den Endpunkt in der
aktuellen Doku bzw. `openapi.json` der App nachlesen** (WebFetch) und im Code mit Link belegen.

## Neue Nextcloud-Funktion anbinden – Vorgehen

1. Capability und Mindestversion klären; was passiert, wenn die App fehlt (UI-Zustand)?
2. Datenmodell neutral halten (wie `torro_pim`): Nextcloud ist ein Provider, nicht das Modell.
   Offene Standards (CalDAV, CardDAV, WebDAV, IMAP) vor Nextcloud-eigenen APIs.
3. Code in einen Provider-Crate (`torro-pim-caldav` oder neu, z. B. `torro-nextcloud` für OCS) –
   neues Crate nur nach Rückfrage. Der Kern cached, die App liest über IPC.
4. Lesen nie direkt ins Netz aus der UI; Schreiben zuerst an den Server, ETag-geschützt.
5. Fehler sauber unterscheiden: offline, nicht angemeldet/widerrufen, Wartung, gedrosselt,
   Konflikt (412), fehlende App, Serverfehler – jeweils eigener `ProviderError`-Fall bzw. eigene Meldung.

## Testen

- Unit-Tests ohne Netz (Regel aus `CLAUDE.md`): echte Antworten (XML/JSON) als Fixtures unter
  `tests/fixtures/` des Crates ablegen und die Parser damit testen; Netzlogik über Closures/Traits
  austauschbar halten (Vorbild: `poll_until_granted` in `account.rs`).
- Gegen echten Server nur manuell bzw. `#[ignore]`: lokales Nextcloud per
  `docker run -d -p 8080:80 nextcloud` (Admin beim ersten Aufruf anlegen), Basis `http://localhost:8080`.
- Fixtures anonymisieren: keine echten Namen, Mails, Tokens.
