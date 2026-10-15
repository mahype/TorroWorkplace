# Nextcloud: Apps der Standardinstallation

Stand: 2026-10-15. Aktuelle Version: **Nextcloud 35.0.1** (24.09.2026). Gepflegt werden außerdem 34 (34.0.4)
und 33 (33.0.9).

Quelle ist `core/shipped.json` im Server-Repo (Zweige `stable33` bis `stable35`, `master` = 36 dev):
`shippedApps` = im Paket, `defaultEnabled` = nach der Installation aktiv, `alwaysEnabled` = nicht abschaltbar.
Die empfohlenen Apps stehen in `core/src/components/setup/RecommendedApps.vue`.

## Fazit vorweg

- Der Server bringt 57 Apps mit, 51 davon sind sofort aktiv. Das sind Dateien, Teilen, Dashboard,
  Benachrichtigungen, Sicherheit, Verwaltung und die Schnittstellen.
- **Kalender, Kontakte, Mail, Talk, Office und Notizen sind nicht im Paket.** Der Einrichtungs-Assistent
  bietet sie an und lädt sie aus dem App Store.
- **CalDAV, CardDAV und WebDAV (`dav`) sind immer da**, auch ohne Kalender- oder Kontakte-App. Für
  TorroWorkplace heißt das: Kalender, Aufgaben (VTODO) und Kontakte gehen auf jedem Server.
- **Tasks, Deck, Notes und Talk** können fehlen. Ob sie da sind, sagen die Capabilities bzw. `occ app:list`.
- Hoster, AIO und Snap aktivieren oft mehr. Ein echter Server kann von dieser Liste abweichen.
- Was man mit diesen Apps teilen kann – im Server, föderiert, nach außen: [nextcloud-zusammenarbeit.md](nextcloud-zusammenarbeit.md).

## 1. Mitgeliefert und aktiv

🔒 = fest aktiv (`alwaysEnabled`), lässt sich nicht abschalten.

### Dateien und Inhalte

| App-ID | Zweck |
|---|---|
| `files` 🔒 | Dateiverwaltung |
| `files_sharing` | Teilen, öffentliche Links |
| `sharebymail` | Teilen per E-Mail |
| `files_trashbin` | Papierkorb |
| `files_versions` | Dateiversionen |
| `files_downloadlimit` | Download-Limit für Freigabe-Links |
| `files_reminders` | Erinnerungen an Dateien |
| `files_lock` *(ab 34)* | Dateien vorübergehend sperren |
| `files_pdfviewer` | PDF-Anzeige |
| `viewer` 🔒 | Anzeige für Bilder und Videos |
| `text` | Markdown-Editor, gemeinsames Bearbeiten |
| `photos` | Fotos |
| `office` *(ab 34)* | Übersicht der Office-Dokumente (das Bearbeiten macht `richdocuments`) |
| `systemtags` | Schlagwörter an Dateien |
| `comments` | Kommentare an Dateien |
| `related_resources` | Zugehörige Inhalte (Talk-Räume, Deck-Karten …) |
| `recommendations` | Vorschläge zuletzt genutzter Dateien |

### Startseite, Personen, Kommunikation

| App-ID | Zweck |
|---|---|
| `dashboard` | Startseite mit Widgets |
| `activity` | Aktivitäten |
| `notifications` | Benachrichtigungen |
| `nextcloud_announcements` | Ankündigungen von Nextcloud |
| `user_status` | Eigener Status |
| `weather_status` | Wetter auf dem Dashboard |
| `profile` 🔒 | Profilseite |
| `circles` | Teams (früher Kreise) |
| `contactsinteraction` | Zuletzt kontaktierte Personen |
| `firstrunwizard` | Begrüßung beim ersten Anmelden |

### Sicherheit und Anmeldung

| App-ID | Zweck |
|---|---|
| `bruteforcesettings` | Brute-Force-Schutz, Ausnahmeliste |
| `password_policy` | Passwortregeln |
| `twofactor_totp` *(ab 34 aktiv)* | Zwei-Faktor mit TOTP |
| `twofactor_backupcodes` 🔒 | Backup-Codes für Zwei-Faktor |
| `oauth2` 🔒 | OAuth2-Clients |
| `privacy` | Datenschutz-Übersicht |

### Verwaltung

| App-ID | Zweck |
|---|---|
| `settings` 🔒 | Einstellungen |
| `theming` 🔒 | Eigenes Aussehen |
| `appstore` 🔒 *(ab 34 eigene App)* | App Store |
| `app_api` | Rahmen für externe Apps (ExApps, oft KI) |
| `serverinfo` | Systemauslastung, Statistik |
| `logreader` | Protokoll ansehen |
| `updatenotification` | Hinweis auf Updates |
| `support` | Support- und Abo-Hinweise |
| `survey_client` | Freiwillige Nutzungsstatistik |

### Schnittstellen und Föderation

| App-ID | Zweck |
|---|---|
| `dav` 🔒 | WebDAV, CalDAV, CardDAV |
| `provisioning_api` 🔒 | OCS-API für Nutzer und Gruppen |
| `sharing` 🔒 *(ab 35)* | API und occ-Befehle für Freigaben |
| `federation` | Vertrauenswürdige Server |
| `federatedfilesharing` 🔒, `cloud_federation_api` 🔒 | Teilen über Servergrenzen |
| `lookup_server_connector` 🔒 | Globales Adressbuch |
| `workflowengine` 🔒 | Flow (Regeln, Automatisierung) |
| `webhook_listeners` | Webhooks bei Ereignissen |

## 2. Mitgeliefert, aber aus

| App-ID | Zweck |
|---|---|
| `files_external` | Externe Speicher (SMB, S3, SFTP …) |
| `encryption` | Serverseitige Verschlüsselung |
| `user_ldap` | LDAP / Active Directory |
| `admin_audit` | Audit-Protokoll |
| `suspicious_login` | Erkennt verdächtige Anmeldungen |
| `twofactor_nextcloud_notification` | Zwei-Faktor über ein anderes angemeldetes Gerät |

## 3. Vom Einrichtungs-Assistenten empfohlen

Nicht im Paket. Der Assistent bietet sie nach der Installation an (vorausgewählt) und lädt sie aus dem App Store.
Server plus diese Apps heißt **Nextcloud Hub**.

| App-ID | App |
|---|---|
| `calendar` | Kalender |
| `contacts` | Kontakte |
| `mail` | Mail |
| `spreed` | Talk |
| `richdocuments` (+ `richdocumentscode`) | Nextcloud Office mit eingebautem Collabora (CODE) |
| `notes` | Notizen |

## 4. Weder mitgeliefert noch empfohlen

Für TorroWorkplace wichtig, nur über den App Store:

- **Tasks:** nur die Weboberfläche. Die Aufgaben selbst laufen über CalDAV (VTODO) und brauchen nur `dav`.
- **Deck**
- **Forms**, **Polls**, **Bookmarks**, **News**, **Cookbook** u. a.

## Änderungen zwischen den Versionen

| Version | Neu |
|---|---|
| 34 | `appstore` (eigene App), `office` und `files_lock` mitgeliefert und aktiv; `twofactor_totp` jetzt aktiv |
| 35 | `sharing` (API und occ für Freigaben), fest aktiv |
| 36 (dev) | bisher keine Änderung gegenüber 35 |

## Prüfen auf einem echten Server

```bash
occ app:list
```

Ohne Shell-Zugang: `GET /ocs/v2.php/cloud/capabilities` (mit `OCS-APIRequest: true`) zeigt die aktiven Apps,
die Fähigkeiten melden, z. B. `spreed`, `notes`, `deck`.
