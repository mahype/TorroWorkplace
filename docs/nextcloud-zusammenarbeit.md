# Nextcloud: Zusammenarbeit, Teilen und Föderation

Stand: 2026-10-15. **Recherche**, noch kein Konzept und keine Aufgabe. Aktuell ist Nextcloud 35
(Hub 26 Summer, seit 16.09.2026); gepflegt werden außerdem 34 (Hub 26 Spring) und 33 (Hub 26 Winter).
Unsere eigenen Server liefen zum Stand auf **33.0.9** und **34.0.3** (`/status.php`).

Frage: Was können Nutzer einer Nextcloud mit anderen teilen – im eigenen Server, mit Nutzern anderer
Nextcloud-Server und ganz nach außen? Welche Apps des Standardpakets beteiligt sind, steht in
[nextcloud-apps.md](nextcloud-apps.md). Das Teilen *zwischen den Apps der Suite* (Kontakt per Mail
usw.) ist ein eigenes Thema: [teilen.md](teilen.md).

## Fazit vorweg

- **Drei Reichweiten:** im eigenen Server (Nutzer, Gruppen, Teams), zu anderen Nextcloud-Servern
  (Föderation über die Federated Cloud ID `benutzer@server`) und ganz nach außen (Links, E-Mail, Gäste).
- **Dateien und Talk** gehen über Servergrenzen seit Jahren zuverlässig.
- **Kalender, Deck und Teams** sind seit Version 33 föderiert, also neu. Teams über Server hinweg
  hakt laut Forum noch. Vor dem Einsatz an einem Testfall prüfen.
- **Adressbücher und Notizen** sind nicht föderiert (Notizen nur über den Umweg Dateiordner).
- Admins können fast alles vorschreiben oder abschalten. Was ein Server erlaubt, sagen die
  Capabilities (`files_sharing`, `federation`, `spreed` …), nicht die Versionsnummer.

## 1. Im eigenen Server

| Was | Teilen mit | Rechte und Optionen |
|---|---|---|
| **Dateien, Ordner** | Nutzer, Gruppen, Teams, Talk-Unterhaltungen | Lesen, Bearbeiten, Anlegen, Löschen, Weiterteilen; Ablaufdatum, Notiz; der Empfänger nimmt automatisch an oder bestätigt jede Freigabe (Einstellung „Persönlich › Teilen“) |
| **Kalender** | Nutzer, Gruppen, Teams | nur lesen oder bearbeiten |
| **Aufgaben** | wie Kalender (Aufgabenlisten sind CalDAV-Kalender) | wie Kalender |
| **Adressbücher** | Nutzer, Gruppen | nur lesen oder bearbeiten |
| **Deck-Boards** | Nutzer, Gruppen, Teams | Bearbeiten, Teilen, Verwalten |
| **Notizen** | über den Notizen-Ordner in Dateien | wie Dateien |
| **Talk** | Gruppenchats, Anrufe, Dateien im Chat | ab 35 auch Dateien als bearbeitbar teilen |

Hinweise:
- Benennt der Besitzer eine geteilte Datei um, sieht der Empfänger weiter den alten Namen.
- „Andere mit Zugriff“ zeigt, wer über einen übergeordneten Ordner Zugriff hat.

### Teams (früher „Circles“)

Selbst gebildete Gruppen. Mitglieder können Nutzer, Gruppen, E-Mail-Adressen und andere Teams sein;
geteilt wird einmal mit dem ganzen Team.

Neu in **35 (Hub 26 Summer):** eine eigene App „Teams“ mit Übersicht. Jedes neue Team bekommt einen
Arbeitsbereich mit Startseite, eigenem Speicher (Quota setzt der Admin) und Ressourcen. Collectives
und Deck-Boards, die ein Team anlegt, gehören dem Team, nicht nur der Person, die sie angelegt hat.
Teams lassen sich über SCIM/OIDC von außen verwalten.

### Neues Teilen ab 35

- Die App `sharing` (fest aktiv) bringt eine neue API und `occ`-Befehle für Freigaben. Der neue
  Teilen-Dialog („unified sharing“) startet in der Seitenleiste von Dateien und soll auf andere Apps
  übergehen. Alte Freigaben bleiben unberührt.
- **Einmal-Passwort (OTP)** für Freigaben, standardmäßig per E-Mail, über die API erweiterbar (SMS).
- **Governance:** Prüfung von Freigaben gegen Regeln der Organisation (auch per `occ`).

## 2. Zu anderen Nextcloud-Servern (Föderation)

Adressiert wird mit der **Federated Cloud ID** `benutzer@server`. Der Empfänger bekommt eine
Benachrichtigung und nimmt an oder lehnt ab. Die Daten bleiben beim Besitzer; der andere Server
greift live darauf zu (bei Dateien per WebDAV).

| Was | Seit | Stand und Einschränkungen |
|---|---|---|
| **Dateien, Ordner** | lange | ausgereift. Passwort und Ablaufdatum gelten auch dort (seit 22). Einen öffentlichen Link kann man mit „Zu meiner Nextcloud hinzufügen“ einbinden. Bis zum ersten Öffnen steht die Freigabe als „(remote)“ da. |
| **Talk** | Hub 8 (Talk 19), Anrufe ab Hub 9 | ausgereift. Chats und Anrufe über Servergrenzen; der Admin muss die Föderation in Talk einschalten. |
| **Kalender** | 33 | neu. Termine geteilter Kalender sind für alle Beteiligten sichtbar. Annehmen/Ablehnen kommt mit 35. **Offen:** ob auch Schreiben geht – die Release-Notes sagen nur „verfolgen“. |
| **Deck** | 33 | neu. Externe sehen das Board und legen Karten und Stapel an. |
| **Teams** | 33 | neu, wackelig. Teams mit Mitgliedern anderer Organisationen, die ihre eigene Kennung behalten. Im Forum (März 2026) klappt das Hinzufügen externer Nutzer nicht, ein Schalter fehlt. |
| **Tables** | 35 | neu |
| **Adressbücher, Notizen** | – | nicht föderiert |

### Vertrauenswürdige Server

Tragen sich die Admins zweier Server gegenseitig ein, gleichen die Server ihre Nutzerverzeichnisse
ab (`occ federation:sync-addressbooks`, meist per Cron). Dann vervollständigt der Teilen-Dialog
externe Namen, man muss die ID nicht kennen. Eine Ampel zeigt den Zustand: rot = keine Verbindung,
gelb = verbunden ohne Abgleich, grün = abgeglichen (beide Seiten vertrauen sich).

### Voraussetzungen

- App `federation` aktiv, Senden und Empfangen in den Admin-Einstellungen „Teilen“ erlaubt.
- Der Server muss unter einem vollständigen Domainnamen von außen erreichbar sein, denn Freigaben
  tragen die URL, mit der sich der Nutzer angemeldet hat (keine LAN-IP).
- HTTPS empfohlen; `trusted_domains` richtig gesetzt. Für große Dateien ggf.
  `davstorage.request_timeout` (Standard 30 s) erhöhen.

### Open Cloud Mesh (OCM)

Die Föderation baut auf **OCM** auf, einem offenen Standard (IETF-Entwurf). Grundsätzlich können
andere Systeme mitmachen (ownCloud, OpenCloud, CERNBox). Wie weit das trägt, hängt von der Gegenseite
ab. Die Kalender-Föderation läuft ebenfalls über OCM (ein DAV-basierter Kalender-Anbieter).

## 3. Ganz nach außen

- **Öffentlicher Link:** nur lesen, hochladen und bearbeiten, oder **File Drop** (nur hochladen, ohne
  den Inhalt zu sehen). Dazu Passwort, Ablaufdatum, Download verbergen (erschwert, verhindert nicht),
  Notiz, mehrere Links mit verschiedenen Rechten. Dateien lassen sich per Link gemeinsam bearbeiten.
- **Per E-Mail** (`sharebymail`): der Link geht direkt an eine Adresse; ab 35 mit Einmal-Passwort.
- **Download-Limit** für Links (`files_downloadlimit`).
- **Kalender:** öffentlich veröffentlichen (nur lesen), auch zum Einbetten.
- **Talk:** Gäste per Link in Unterhaltungen und Anrufe; ab 35 können Kanäle Gäste einladen. Die
  Vorlage „Classified“ schaltet öffentliche Links und Gäste bewusst ab.
- **Gastkonten:** App „Guests“ (App Store) für eingeschränkte Konten.

Admins können Passwort und Ablaufdatum erzwingen, öffentliche Uploads verbieten und die Föderation
ganz abschalten.

## 4. Bezug zu TorroWorkplace

- **Dateien:** OCS-Sharing-API `/ocs/v2.php/apps/files_sharing/api/v1/shares`. Die Art der Freigabe
  steht in `shareType`: Nutzer (0), Gruppe (1), Link (3), E-Mail (4), föderiert (6), Team (7),
  Talk-Raum (10). Offene föderierte Einladungen: `…/api/v1/remote_shares/pending`, annehmen per
  `POST …/pending/<id>`. Ab 35 kommt die neue `sharing`-API dazu; vor einer Umsetzung deren
  `openapi.json` lesen.
- **Kalender:** geteilte Kalender sieht der Kern schon heute über CalDAV (`owner-principal`,
  schreibgeschützt erkennbar an `current-user-privilege-set`). Wie föderierte Kalender (ab 33) über
  CalDAV erscheinen, ist **nicht geprüft**.
- **Mögliche Funktionen** (nur Ideen, kein Plan): „Per Link teilen“ als Ziel im Teilen-Blatt aus
  [teilen.md](teilen.md); offene Einladungen (föderierte Freigaben, Kalender) im Eingang annehmen.
  Jede davon bräuchte Oberfläche, API-Befehl und MCP-Tool; Links und Freigaben nach außen wären
  „nach außen“ und bräuchten eine Freigabe durch den Menschen.

## Offen

- Schreibrechte bei föderierten Kalendern (33/34) an zwei Testservern prüfen.
- Föderierte Teams erneut prüfen, sobald 35.x stabil ist.
- Ob Talk-Föderation auf unseren Servern eingeschaltet ist (Capabilities `spreed` → `federation`).

## Quellen

- Nextcloud User Manual, File Sharing:
  https://docs.nextcloud.com/server/latest/user_manual/en/files/sharing.html
- Nextcloud Admin Manual, Federated Cloud Sharing:
  https://docs.nextcloud.com/server/latest/admin_manual/configuration_files/federated_cloud_sharing_configuration.html
- heise, Hub 26 Winter erweitert Föderation: https://heise.de/-11186171
- Nextcloud, Pressemitteilung Hub 26 Winter: https://nextcloud.com/de/blog/press_releases/nextcloud-hub26-winter/
- Nextcloud, Blog Hub 26 Summer: https://nextcloud.com/blog/nextcloud-hub26-summer/
- Erste Beta von Hub 26 Summer (v35), Changelog:
  https://help.nextcloud.com/t/first-beta-release-of-nextcloud-hub-26-summer-v35/248008
- Forum, föderierte Teams: https://help.nextcloud.com/t/can-not-configrue-federated-teams-circles/240423
- IETF 126, OCM in Nextcloud:
  https://datatracker.ietf.org/meeting/126/materials/slides-126-ocm-open-cloud-mesh-in-nextcloud-where-we-are-who-built-it-where-were-headed-00
