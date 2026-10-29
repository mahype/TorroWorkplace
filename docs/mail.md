# Mail in der Suite: Konten, Mail-App, Rechte

Stand: 2026-10-11. **Konzept zur Besprechung.** Beantwortet die offene Frage „Mail-Client der
Suite“ aus [roadmap.md](roadmap.md#offene-fragen). Die Entscheidungen, die noch offen sind,
stehen am Ende.

## Ausgangslage

- **Workplace** kennt genau ein Konto: Nextcloud (`AccountKind::Nextcloud`, `accounts.json`,
  Geheimnis im Schlüsselbund). `PimService` hält einen einzigen Zustand, `stored()` nimmt das
  erste Konto. Die Kennungen von Sammlungen sind CalDAV-URLs ohne Bezug zum Konto. Ein Trait für
  mehrere Anbieter (`torro_pim::Provider`) gibt es, aber nichts setzt ihn um.
- **Rechte je Agent** stehen im Dienst (A.1/A.2): eigener Schlüssel je Client, Stufe je Bereich,
  Auswahl von Kalendern und Aufgabenlisten. Es fehlen die Seiten in der Steuerzentrale (A.6–A.8)
  und die Freigaben (A.3).
- **TorroMail** ist eine eigenständige Zugriffsschicht für Agenten, ausdrücklich kein
  Mail-Programm: eigener IMAP-Client (synchron, IMAP4rev1-Teilmenge), SMTP, Autodiscovery,
  SQLite-Cache mit FTS5, 15 MCP-Tools, macOS-App (SwiftUI) und TUI zum Einrichten. Kein IDLE,
  kein Hintergrund-Sync, nichts zu Kalender oder Kontakten.

## Was in einem Mail-Konto steckt

Eine Richtigstellung vorweg: **IMAP transportiert nur Mail.** Termine und Aufgaben liegen nicht
im IMAP-Postfach, sondern beim selben Anbieter hinter einem anderen Protokoll. Ein „Mail-Konto“
ist in Wahrheit ein Zugang, hinter dem mehrere Dienste stehen können:

| Dienst | Protokoll | Was es bringt | Für uns |
|---|---|---|---|
| Mail lesen | IMAP | Ordner, Nachrichten, Flags, Schlagworte (`$Label1`, Gmail-Labels), Quota | Kern der Mail-App |
| Mail senden | SMTP | Versand | Kern der Mail-App |
| Push | IMAP IDLE / NOTIFY | Neue Mail sofort statt Abfrage | Sync im Kern |
| Filter, Abwesenheit | ManageSieve (Port 4190) | Serverseitige Regeln, Abwesenheitsnotiz | später, sehr nützlich |
| Kalender, Aufgaben | CalDAV (VEVENT, VTODO) | Dasselbe wie bei Nextcloud | haben wir schon (`torro-pim-caldav`) |
| Kontakte | CardDAV | Adressbuch, Empfänger-Vorschläge | später, eigene App |
| Einladungen in Mails | iMIP (`text/calendar`, REQUEST/REPLY/CANCEL) | Termin annehmen/ablehnen aus der Mail | Brücke Mail ↔ Kalender |
| Visitenkarten in Mails | `text/vcard` | Kontakt übernehmen | später |

Sonderfälle, die wir **nicht** unterstützen: Kolab legt Termine als Mails in IMAP-Ordnern ab,
Apple Notes nutzt einen IMAP-Ordner „Notes“, Outlook hat lokale Kalender „(Nur dieser
Computer)“. Google und Microsoft haben eigene Schnittstellen (Google bietet CalDAV mit OAuth,
Microsoft 365 nur Graph/EWS). JMAP (Fastmail, Stalwart) bündelt Mail, Kalender und Kontakte in
einem Protokoll, ein Kandidat für später.

**Folge für das Kontomodell:** Ein Konto ist eine Anmeldung mit **Diensten**. Beim Einrichten
eines Mail-Kontos sucht der Kern zusätzlich nach CalDAV/CardDAV derselben Domain (RFC 6764:
SRV `_caldavs._tcp`, `/.well-known/caldav`) und bietet gefundene Dienste an, vorausgewählt.
Nextcloud ist dann einfach ein Konto mit den Diensten Kalender, Aufgaben (später Kontakte,
Dateien), aber ohne Mail.

```
Konto (Anmeldung, ein Geheimnis)
 ├─ Mail      IMAP + SMTP          → Mail-App
 ├─ Kalender  CalDAV VEVENT        → Kalender-App
 ├─ Aufgaben  CalDAV VTODO         → Aufgaben-App
 └─ Kontakte  CardDAV              → später
```

## Linke Leiste für Konten (wie Mailbird)?

Mailbird hat links eine schmale Spalte mit einem Symbol je Konto, daneben die Ordner, dann Liste
und Lesebereich. Unsere Apps haben links schon die Seitenleiste aus `torro_gui::shell` (232 px,
eingeklappt 52 px, im schmalen Modus als Schublade). Eine Kontenleiste käme **zusätzlich davor**.

**Entschieden (2026-10-11): keine eigene Kontenleiste, in keiner App.** Stattdessen gilt überall
dasselbe Muster:

1. **Alle Konten zusammen ist der Normalfall.** Der Kalender legt alle Kalender übereinander, die
   Aufgaben zeigen alle Listen, die Mail zeigt einen gemeinsamen Posteingang. Wer nur ein Konto
   hat, sieht vom Konzept „Konto“ gar nichts.
2. **Konten sind Gruppen in der vorhandenen Seitenleiste** (`sidebar_group_in`): „Calendars ▾“
   wird zu „Nextcloud ▾ / mailbox.org ▾“ mit den Kalendern darunter; in der Mail-App hat jedes
   Konto eine aufklappbare Gruppe mit seinen Ordnern. Gruppen erscheinen erst ab zwei Konten.
3. **Trennung nach Konto geht über Fenster, nicht über Knöpfe.** Ein Fenster kann auf ein Konto
   festgelegt werden (`--account <id>`, wie das geplante `--window` in [settings.md](settings.md)),
   und die Workspace-Vorlage merkt sich das. Wer Arbeit und Privat trennen will, legt zwei
   Mail-Kacheln nebeneinander. Das ist der Tiling-Weg und passt zu „eine App, eine Frage, ein
   Fenster“.

Warum keine Leiste:

- Im Kalender und bei Aufgaben ist ein Umschalter je Konto falsch: Er blendet Termine anderer
  Konten aus, genau die, die man für die Planung braucht.
- Sie kostet in jeder Kachel 52–64 px Breite, und Breite ist im Tiling knapp.
- Bei ein bis zwei Konten ist sie Dekoration.
- Mailbird braucht sie, weil es ein All-in-one-Programm mit eingebetteten Fremd-Apps ist. Genau
  das ersetzt bei uns der Tiling-Manager ([vision.md](vision.md)).

Gleich bedienbar bleibt es trotzdem: Konto-Gruppen, Konto-Farbe und `--account` funktionieren in
allen Apps gleich.

## Die Mail-App

> **Entschieden 2026-10-12:** keine eigene App. Was hier steht (Größenklassen, Schreibfenster,
> Einladungen), gilt für die Mail-Tiefe im Eingang `torroinbox` ([eingang.md](eingang.md)).
>
> **Aktualisierung 2026-10-21 (#267):** Die untenstehende Aussage, es gebe keine eigenen
> Kontoeinstellungen im Mail-Fenster, ist durch [settings.md](settings.md) überholt.
> Mail-spezifische Einstellungen liegen hinter dem Zahnrad des Eingang/Mail-Fensters;
> Backup & Restore liegt dort unter Konten → Konto ([Konzept](mail-backup.md)).

Arbeitsname `torromail` (siehe offene Entscheidungen). Eigener Prozess, Iced, `torro_gui::shell`,
spricht nur über IPC mit dem Dienst wie Kalender und Aufgaben.

| Größe | Aufbau |
|---|---|
| **Breit** (≥ 1024) | Seitenleiste (oben „Inbox“ für alle Konten, „Flagged“, „Drafts“, darunter je Konto die Ordner) · Nachrichtenliste · Lesebereich. Drei Spalten wie Mailbird, nur ohne Kontenleiste |
| **Mittel** | Seitenleiste eingeklappt (52 px) · Liste · Nachricht im Paneel über der Liste (Muster U.3/U.5) |
| **Schmal** | Liste; Nachricht öffnet sich darüber mit Zurück |
| **Mini / Leiste** | Anzeige, keine Mini-App: ungelesen je Konto, die drei neuesten Absender. Frage: „Wer wartet auf mich?“ |

- **Schreiben in einem eigenen Fenster.** Im Tiling kachelt es sich neben die Liste, man kann
  beim Schreiben nachlesen. Im Fenster-Modus als Paneel.
- **Einladungen** (iMIP) zeigen oben in der Nachricht „Accept / Maybe / Decline“; die Antwort
  geht per Mail zurück (iTIP `REPLY` über das Konto, Schalter „Nach außen“ wie beim Senden), der
  Termin landet in dem Kalender, der die frühere Kopie hält, sonst im ersten beschreibbaren (E8.3b,
  Einzelheiten in [api.md](api.md), Fußnote 16). Das gehört in den Kern, weil Mail und Kalender
  dort zusammenkommen.
- **Was die App nicht hat:** eigene Kontoeinstellungen (die liegen in der Steuerzentrale),
  Plugins, eingebettete Fremd-Apps, HTML-Editor im ersten Schritt (Text und einfache
  Formatierung reichen).

## Ordner: Entscheidungen statt Optionen

**Warum TorroMail Ordner zuordnen lässt:** Bis September hat TorroMail beim Versand gar keine
Kopie in „Gesendet“ abgelegt und Entwürfe in einen fest eingetragenen Ordner „Drafts“
geschrieben. Mit 3330beb wurde „Gesendet finden“ zur Bedingung für den Versand: Lieber vorher
abbrechen, als nach dem SMTP-Versand festzustellen, dass die Kopie fehlt, und dann doppelt zu
senden. Wo ein Server keine SPECIAL-USE-Kennzeichen liefert und kein bekannter Name passt, ging
deshalb kein Versand mehr. Die manuelle Zuordnung von fünf Rollen (Entwürfe, Gesendet, Archiv,
Spam, Papierkorb) war der Ausweg. Die Erkennung selbst ist gut und bleibt; zu viel ist der
immer sichtbare Abschnitt zum Zuordnen.

**Für die Suite:**

1. Die Rollen werden immer automatisch bestimmt: `LIST`-Kennzeichen, `LIST (SPECIAL-USE)`,
   `XLIST`, dann bekannte und übersetzte Namen (Liste aus TorroMail übernehmen).
2. **Fehlt ein Ordner, legt der Kern ihn an**, mit dem Präfix aus `NAMESPACE`
   (`INBOX.Sent`). Das tun Thunderbird und Apple Mail auch. Danach blockiert nichts mehr den
   Versand. Die Regel „Kopie fehlgeschlagen → nie erneut senden“ bleibt.
3. Anbieter, die Gesendetes selbst ablegen (Gmail, Outlook.com), bekommen keine zweite Kopie.
4. Ein Auswahlfeld gibt es nur im **Ausnahmefall**: Gibt es zwei Kandidaten mit Inhalt (etwa
   „Sent“ und „Gesendete Objekte“), fragt die Kontoseite einmal, welcher gilt. Sonst kein
   Abschnitt „Ordner“.

Was wir aus TorroMail außerdem **nicht** übernehmen: Regeln je Ordner für Agenten, den Schalter
`per_folder`, Lesestufen „nur Kopfzeilen / ganze Nachricht / mit Anhängen“ für Agenten
(für sie reicht Lesen ja/nein je Konto), Dock- und Menüleisten-Schalter. Seit #263
ist die Cache-Strategie je Konto unter Mail-Einstellungen › Storage & index wählbar:
nur Kopfzeilen, Mailtexte der letzten 90 Tage oder alle Mailtexte offline. Der Suchindex bleibt
unabhängig vom Lesecache; Anhänge haben eigene Vorladeoption und Aufbewahrung.
Im Modus „alle Mailtexte offline“ hält der Dienst seit #267 zusätzlich die vollständigen
RFC-822-Originalbytes mit Prüfsumme in einer unabhängigen SQLite-Tabelle je Konto. Der
Cache-Auftrag ergänzt fehlende Originale auch für bereits zwischengespeicherte Texte; ein
schon vorhandenes Original wird beim erneuten Öffnen weder geholt noch neu geschrieben.
Das Originaldepot überlebt das Löschen von Textcache und veralteten Headern und benötigt
entsprechend dauerhaft Speicherplatz. Verlässt eine Mail den Server (gelöscht, verschoben,
Ordner neu nummeriert oder entfernt), wird ihr Original als „nicht mehr auf dem Server“
markiert, aus lokalen Backups ausgelassen und nach 30 Tagen beim regulären Wartungslauf
nach dem Sync gelöscht; alle übrigen Originale bleiben bis zum Entfernen des Kontos. Kann
ein Original nicht erfasst werden, laufen Sync und Suchindex trotzdem weiter. Nur diese
Originale sind Quelle eines lokalen ZIP-Backups; fehlende Originale werden ausgewiesen und
machen das Archiv unvollständig ([Konzept](mail-backup.md)).

## Architektur: Mail im Kern

Wie Kalender und Aufgaben: **Konten, Sync, Cache und Prüfung liegen im Dienst**, die App zeigt
nur an. Das beantwortet „Liegen die Mail-Konten im Kern?“ mit Ja. Neue Crates:

| Crate | Inhalt |
|---|---|
| `torro-mail` | Typen ohne Ein-/Ausgabe: Konto-Dienst Mail, Ordner, Rolle, Nachricht (Kopf/Inhalt), Flags, Entwurf. Wie `torro-pim` |
| `torro-mail-imap` | IMAP und SMTP, Ordner-Rollen, Autodiscovery (Ablauf aus TorroMail: Tabelle → MX → Autoconfig → ISPDB → SPF → SRV) |
| `torro-store` (oder in `torro-pim`) | SQLite mit FTS5 für Mail und den geplanten PIM-Cache, ein gemeinsames `rusqlite` |

**Bibliotheken statt eigenem IMAP-Client.** TorroMails Client ist bewusst klein und synchron.
Ein volles Mail-Programm braucht IDLE, CONDSTORE/QRESYNC, viele Verbindungen und tokio. Vorschlag:
`async-imap` (oder `imap-codec`/`imap-next`), `mail-parser`, `mail-builder`, `lettre` für SMTP,
alle über rustls, das schon im Baum ist. Aus TorroMail übernehmen wir das **Wissen**, nicht den
Code: Discovery-Reihenfolge, Ordnernamen, modified UTF-7, die Versandregel, die
Vertrags-Fixtures als Testfälle.

**Sync:** IDLE auf dem Posteingang je Konto, die übrigen Ordner per CONDSTORE alle paar Minuten.
Erst Kopfzeilen, Inhalte der jüngsten Nachrichten im Voraus, der Rest beim Öffnen. Neue Mail →
Benachrichtigung über den Kern (`notify-rust` ist da).

**Kontingent und Größen (#314, DS.1):** Der Sync speichert mit den Kopfzeilen die Größe jeder Mail
(`RFC822.SIZE`) in `messages.size` (Mail-Speicher Schema 12; Zeilen von vorher haben `NULL`). Fehlende
Größen füllt er je Lauf und Ordner nach, höchstens 2000 in Stapeln zu 500, auch für einen Ordner,
der sich nicht geändert hat; so bekommen alte Konten ihre Größen über einige Läufe ohne
Lastspitze. Ein Ordner nennt seine Größe auf dem Server (`server_bytes` in `mail.account_stats`)
erst, wenn alle seine Mails eine haben; nennt die Antwort des Servers für eine Mail keine Größe (ohne
`RFC822.SIZE` oder gar nicht), zählt sie mit 0 und wird nicht wieder gefragt. Einmal je Sync fragt der Dienst das Kontingent
(`GETQUOTAROOT "INBOX"`, RFC 9208, Ressource `STORAGE` in Einheiten zu 1024 Byte), nur bei Servern
mit `QUOTA`, und speichert es unter `cache_control.key = 'quota'`; ein Server ohne `QUOTA` löscht
ein altes. Die Statistik fragt den Server also nie selbst und geht auch offline. Dazu kommen
Mail je Monat (nach Datum in UTC) und die behaltenen Originale je Ordner. Die Oberfläche zeigt das
auf der Seite des Kontos (Mail › Accounts); ohne Kontingent steht dort die Summe der Ordner ohne
„of …“.

**GitHub-Mail (E5.6):** Der Mail-Speicher je Konto (Schema 4) hält neben `List-Id`,
`List-Unsubscribe`, `Precedence` und `Auto-Submitted` auch `X-GitHub-Reason`
(`MessageTraits.github`, nur Kleinbuchstaben und `_`). Eine Mail gilt als GitHub-Mail zu einem
Issue oder PR nur, wenn alle drei passen: `From: notifications@github.com` mit `X-GitHub-Reason`,
`List-Id: <repo.owner.github.com>` und `Message-ID: <owner/repo/(pull|issues)/<n>…@github.com>`
mit demselben Repo. `In-Reply-To` wird dafür nicht gespeichert und nicht gelesen: Jede Mail von
GitHub nennt Repo und Nummer schon in ihrer eigenen `Message-ID`. Der Eingang ordnet sie dann dem
GitHub-Eintrag zu ([eingang.md](eingang.md), Quelle GitHub); Erledigt dort markiert sie gelesen
(`\Seen`), wie bei einem Mail-Eintrag.

**Vorarbeit, die auch ohne Mail nötig ist:** mehrere Konten und Kontotypen. `AccountKind` bekommt
`Mail`, `PimService` wird nach Konto geschlüsselt, Sammlungs-Kennungen bekommen das Konto vorne
dran (`<account-id>/<url>`), `PimClient::account()` liefert eine Liste. Das löst auch das
Ein-Konto-Limit bei Nextcloud.

## Eigene Server und Zertifikate

Entschieden 2026-10-22 (#284, Aufgaben M11 in [tasks/mail.md](tasks/mail.md)): Ein Mail-Konto nimmt jeden Server,
den ein Mensch einträgt, wie er ist – beim Anbieter, im lokalen Netz (NAS, eigener Mail-Server) oder auf diesem
Rechner (`localhost`, etwa eine Bridge). Es gibt keinen Schalter dafür. Das ist sicher, weil Server eines
Mail-Kontos nur über `accounts.connect_mail` hineinkommen, das nur der App-Schlüssel schicken darf: Kein Agent kann
ein Konto auf ein Ziel im lokalen Netz richten ([api.md](api.md), „Was nie nach außen geht“, Punkt 5). Die
Dienstsuche (`discover_services`), die Anmeldung reiner CalDAV-Konten (`connect_caldav`) an einer von ihr
gefundenen Adresse und `torro-htmlview` bleiben bei „nur öffentliche Adressen“.

- **Verbinden:** Der Host wird je Verbindung einmal aufgelöst; IMAP, SMTP und der Kalenderteil verbinden sich genau
  mit diesen Adressen, der Name dient nur TLS (wie seit M2.7b).
- **Zertifikate:** Vertraut wird dem System und, je Konto, genau den Zertifikaten, deren SHA-256 ein Mensch
  bestätigt hat (`MailAccountConfig::trusted_certificates`, Format `AB:CD:…`). Ein bestätigtes Zertifikat gilt ohne
  Prüfung von Name und Ablauf, denn selbst signierte nennen oft einen anderen Namen; die Signaturen des Handshakes
  prüft rustls trotzdem, wer nur das Zertifikat vorzeigt, kommt nicht durch.
- **Rückfrage:** Zeigt ein Server beim Verbinden ein unbekanntes Zertifikat, antwortet der Kern mit
  `untrusted-certificate` (Dienst, Host, Fingerabdruck), bevor das Passwort hinausgeht. Die Steuerzentrale fragt
  „Trust this certificate?“; liegt eine Adresse des Hosts im öffentlichen Internet (`public`), warnt sie deutlich,
  denn bei einem Anbieter heißt ein unbekanntes Zertifikat meist, dass jemand mitliest (M11.4).
- **Merken:** Gespeichert werden nur Zertifikate, die ein Server bei dieser Anmeldung gezeigt hat, höchstens acht,
  das älteste fällt heraus. Jedes erneute Verbinden („Server settings…“, „Change password…“) räumt so nicht mehr
  benutzte weg; lief CalDAV dabei nicht mit, bleiben die übrigen gespeicherten, sie können CalDAV gehören. Ein
  bestätigtes Zertifikat gilt für alle Server des Kontos; wechselt ein Mensch den Host, gilt es also auch für den
  neuen, bis ein Verbinden mit CalDAV aufräumt.
- **Wechsel:** Zeigt der IMAP- oder SMTP-Server später ein anderes Zertifikat, endet die Verbindung vor der
  Anmeldung. Das Konto zeigt „The certificate of the mail server changed. Check it in Server settings.“ als Problem, das ein Mensch lösen
  muss, und versucht es trotzdem weiter: Kein Server sperrt das Konto, weil nie ein Passwort ankommt, und ein
  zurückgesetztes Zertifikat heilt sich von selbst. Ein Versand meldet „Nothing was sent“. Der Kalenderteil scheitert
  dann wie bei einem nicht erreichbaren Server, noch ohne eigenen Hinweis auf das Zertifikat.

## Rechte zentral in der Steuerzentrale

Die Abschnitte API und MCP der Steuerzentrale ([api.md](api.md), A.6–A.8, seit #231 getrennt) sind die
**eine** Stelle für alle Rechte von Programmen und Agenten der Suite. TorroMails eigene Rechteverwaltung entfällt. Der Workplace-Weg ist dabei
schon besser: In TorroMail hängen die Rechte am Konto, alle Agenten mit Zugang zu einem Konto
dürfen dasselbe. Im Workplace hängen sie am Agenten.

Pro Agent (Karte je Client, eigener Schlüssel) wird eingestellt:

| Bereich | Stufen | Auswahl |
|---|---|---|
| Calendar | none / read / write / full | Kalender (gibt es) |
| Tasks | none / read / write / full | Listen (gibt es) |
| **Mail** (neu) | none / read / write / full | **Konten** (neu, `Selection`) |
| **Senden** (neu, Schalter unter Mail) | never / with approval / free | – |

- `read` = suchen, lesen, Anhänge holen. `write` = Entwurf, Flags, verschieben, Archiv.
  `full` = zusätzlich Papierkorb und endgültig löschen.
- **Senden ist kein Schreiben.** Es geht nach außen und lässt sich nicht zurückholen, darum ein
  eigener Schalter, Voreinstellung „with approval“.
- **Freigaben echt machen (A.3/A.8):** Der Request antwortet `pending`; der Mensch sieht in der
  Steuerzentrale und als Benachrichtigung Empfänger, Betreff und Anfang des Textes und entscheidet
  mit `approvals.decide`. Der Agent bekommt **keinen** Code. TorroMail gibt den Code heute dem
  Agenten mit, eine Freigabe durch den Menschen gibt es dort faktisch nicht.
- Vorlagen „Nur lesen / Lesen und schreiben / Voll“ setzen auch Mail; Senden bleibt in allen
  Vorlagen „with approval“.
- Nicht freigegebene Konten verhalten sich wie nicht vorhanden (`not-found`).
- **Schnittstellen-Änderung:** `torro_ipc::api::Rights` bekommt `mail_accounts: Selection` und
  `send: SendPolicy`, `Area` bekommt `Mail`. Neue optionale Felder sind verträglich, brauchen
  aber deine Zustimmung, weil `Rights` festgelegt ist.

## Funktionen: Oberfläche, API, MCP

Namen nach dem Muster `bereich.verb_objekt`, MCP-Tool gleich mit `_`.

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Mail-Konto einrichten | Steuerzentrale › Accounts › Add | `accounts.connect_mail` · Admin | `accounts_connect_mail`: nimmt nur die Adresse, öffnet die Einrichtung in der Steuerzentrale; das Passwort tippt der Mensch dort (wie bei `connect_nextcloud`, Geheimnisse nie über den Agenten) |
| Ordner auflisten | Mail-Seitenleiste | `mail.list_folders` · Read | `mail_list_folders` |
| Suchen / Liste | Mail-Liste, Suche, Filter-Icon im Suchfeld (#336) | `mail.search_messages` · Read | `mail_search_messages` |
| Stand des Suchindex (#136) | Suchfeld: Hinweis „12 400 von 18 000 indiziert“ (Schritt 5 von #136) | `mail.index_status` · Read | `mail_index_status` |
| Bestand und lokaler Cache (#263) | Mail-Einstellungen › Accounts und Storage & index | `mail.account_stats`, `mail.get_cache_policy`, `mail.preview_cache` · Read; `mail.set_cache_policy` · Admin | entsprechende `mail_*`-Tools |
| Cache und Index warten (#263) | Mail-Einstellungen › Storage & index | `mail.start_cache_job`, `mail.control_cache_job` · Write; `mail.cache_job_status` · Read | entsprechende `mail_*`-Tools |
| Nachricht lesen | Lesebereich | `mail.get_message` · Read | `mail_get_message` |
| Unterhaltung | Lesebereich | `mail.get_thread` · Read | `mail_get_thread` |
| Anhang holen | Lesebereich | `mail.get_attachment` · Read | `mail_get_attachment` |
| Ablageziele und Pfad-Vorschau (#290) | Dreieck-Menü am Anhang, Einstellungen › Mail › Attachments | `mail.list_save_targets` · Read | `mail_list_save_targets` |
| Ablageziel anlegen (#290) | Einstellungen › Mail › Attachments | `mail.create_save_target` · Admin | **Never** (Liste Punkt 9: setzt immer einen Ordner) |
| Ablageziel ändern (#290) | Einstellungen › Mail › Attachments | `mail.update_save_target` · Admin; anderer Ordner nur mit dem App-Schlüssel | `mail_update_save_target` (Ordner wie gespeichert) |
| Ablageziel entfernen (#290) | Einstellungen › Mail › Attachments | `mail.remove_save_target` · Admin | `mail_remove_save_target` |
| Anhänge speichern (#290) | Dreieck-Menü, „Save all ▾“, Kontextmenü der Liste, Klick auf den Namen | `mail.save_attachments` · Write; `to: file`/`folder` nur mit dem App-Schlüssel | `mail_save_attachments` (nur Downloads oder ein Ziel) |
| Filter auflisten (#339) | Einstellungen › Mail › Filters | `mail.list_filters` · Read | `mail_list_filters` |
| Filter anlegen, ändern, ein- und ausschalten (#339) | Einstellungen › Mail › Filters; „Create filter…“ an einer Mail, „Create filter from search…“ am Suchfeld | `mail.create_filter`, `mail.update_filter` · Admin | `mail_create_filter`, `mail_update_filter` |
| Filter entfernen (#339) | Einstellungen › Mail › Filters › „Delete filter“ | `mail.remove_filter` · Admin | `mail_remove_filter` |
| Treffer eines Filters zeigen (#339) | Seite des Filters, „In Inbox now“ | `mail.preview_filter` · Read | `mail_preview_filter` |
| Filter auf vorhandene Mail anwenden (#339) | nach dem Speichern „Apply to the {n} mails in Inbox?“ | `mail.apply_filter` · Admin | `mail_apply_filter` |
| Mail als HTML (M9) | Lesebereich der Inbox (Schalter HTML/Text, Taste `v`) | `mail.get_message` mit `include_html` · Read | `mail_get_message` mit `include_html` |
| Flags setzen | Liste, Lesebereich | `mail.set_flags` · Write | `mail_set_flags` |
| Entwurf speichern | Schreibfenster | `mail.save_draft` · Write | `mail_save_draft` |
| Verschieben / Archiv | Liste, Lesebereich | `mail.move_messages` · Write (nach Trash: Delete) | `mail_move_messages` |
| Löschen | Liste, Lesebereich | `mail.delete_messages` · Delete | `mail_delete_messages` |
| Entwurf senden | Schreibfenster | `mail.send_draft` · Write + Senden-Schalter | `mail_send_draft` (antwortet ggf. `pending`) |
| Einladung beantworten (E8.3b) | Leser, Karte | `mail.respond_to_invitation` · Write + Nach außen | `mail_respond_to_invitation` (antwortet ggf. `pending`) |
| Freigabe entscheiden | Steuerzentrale › Approvals, Benachrichtigung | `approvals.decide` · Admin | **Never** (Liste Punkt 3) |
| Agenten-Rechte | Steuerzentrale › MCP (Agenten), › API (Skripte) | `clients.*` · Admin | **Never** (Liste Punkt 2) |

Statt TorroMails Paar `prepare_*` / `confirm_action` gibt es einen Request; ob er sofort läuft
oder auf eine Freigabe wartet, entscheidet der Dienst nach den Rechten des Agenten.

## Anhänge ablegen

Konzept und Entscheidungen: Issue #290, Aufgaben M12 in [`tasks/mail.md`](tasks/mail.md). Ein Anhang lässt sich
öffnen, nach Downloads sichern, über „Save as…“ sichern oder in ein **Ablageziel** legen: einen Ordner mit einem
Schema für Unterordner und Dateinamen (`{year}/{month}`, `{name}.{ext}` …), einer Datumsquelle (Mail oder Datum im
Dateinamen) und Bedingungen, wann es vorgeschlagen wird (Kategorie, Dateityp, Absender).

- Die Ziele gelten für alle Konten gleich und stehen in einer eigenen Datei `<data>/mail/save-targets.json` (0600),
  nicht in `settings.json`; Klick-Aktion (`mail_attachment_click`) und Downloads-Ordner (`mail_downloads_folder`)
  sind Einstellungen.
- Einen Ordner setzt nur ein Mensch (Punkt 9 in `api.md`): Ziele anlegen, einen anderen Ordner eintragen, den
  Downloads-Ordner setzen und `to: file`/`folder` gehen nur mit dem App-Schlüssel.
- Werte aus der Mail werden bereinigt, Ordner entstehen nur durch `/` im Schema, und jeder Unterordner wird nach dem
  Anlegen aufgelöst und muss unter dem Grundordner liegen (auch bei einem Symlink darin). Fehlt der Grundordner,
  meldet der Dienst das (`not-found`).
- Es wird nie überschrieben (außer bei „Save as…“, dort hat der Dialog gefragt): „Keep both“ zählt hoch
  (`Name (2).pdf`), „Skip if identical“ speichert bei gleichem Inhalt nichts. Höchstens 50 MiB je Datei; scheitert
  eine Datei, bleiben die übrigen gespeichert.
- Darf der Dienst nicht in den Ordner schreiben (macOS fragt vor Downloads, Dokumente und Schreibtisch), antwortet
  er `forbidden` mit einem Hinweis auf die Systemeinstellungen.
- Agenten speichern nach der gewöhnlichen Schreibregel; die Freigabe nennt Betreff, Zahl der Dateien und Ziel aus dem
  Speicher des Dienstes und ist an die Mail gebunden.

## Filter

Issue #339. Ein **Filter** ist eine gespeicherte Suche mit einer Aktion, etwa „alles von news@shop.example
archivieren“. Er ersetzt das frühere Ausblenden von Mail (Ansicht „Hidden“, „Hide“, „Show again“): Ausgeblendete Mail
war weg, ohne dass man sah, wohin. Ein Filter legt sie sichtbar dorthin, wo man sie wiederfindet. Im Notification Hub
bleibt das Ausblenden für alle anderen Quellen ([eingang.md](eingang.md#stummschalten-nach-merkmalen)).

**Was ein Filter ist** (`torro_mail::MailFilter`):

- **Bedingung:** eine Suche in der Sprache des Suchfelds (`from:`, `to:`, `subject:`, `has:attachment` …), nicht leer,
  höchstens 500 Zeichen. Ohne `in:`, weil ein Filter nur den Posteingang ansieht, und ohne `filename:`, weil die
  Namen der Anhänge beim Eintreffen nicht bekannt sind.
- **Konto:** ein Mail-Konto oder alle („All accounts“, `account` fehlt).
- **Aktionen**, mindestens eine: verschieben (`file`: Archiv, ein Ordner oder Papierkorb), als gelesen markieren
  (`mark_read`), eine Kategorie des Katalogs setzen (`category`). Ein Ordner braucht ein Konto, muss diesem gehören
  und darf nicht der Posteingang sein. Der Papierkorb ist ein Verschieben, das sich dort zurückholen lässt; endgültig
  löscht ein Filter nie.
- **Schalter:** Ein ausgeschalteter Filter behält seinen Platz und wirkt nicht auf neue Mail.
- **Ablage:** nur im Dienst, in einer eigenen Datei `<data>/mail/filters.json` (0600, wie die Ablageziele), nicht in
  `settings.json` und nicht als Sieve-Regel auf dem Server. Höchstens 200 Filter. Die Kennung hat die feste Form
  `f<n>` und wird nie zweimal vergeben, damit ein alter Verweis nie einen anderen Filter trifft. Fehlt die Datei, gibt
  es keine Filter; ist sie unlesbar, bleibt sie, wie sie ist, und der Dienst antwortet `error` ohne Pfad.

**Wann ein Filter wirkt:**

- Auf **neue Mail im Posteingang**, solange Torro Workplace läuft. Bevor der Abgleich neue Mail im Notification Hub
  ablegt, laufen die eingeschalteten Filter des Kontos über sie, unabhängig von der Einstellung `notify` des Kontos.
  Gefiltert wird, was der Abgleich als neu meldet, auch Mail, die kam, während Torro Workplace nicht lief.
- **Nicht** beim ersten Abgleich eines Ordners (auch nicht, wenn der Server ihn neu nummeriert hat) und nicht, wenn
  der Dienst Verpasstes nur nachholt (`catch_up`, etwa wenn die Ablage im Hub den Meldungen des Abgleichs nicht
  hinterherkam): Diese Mail legt er ab, filtert sie aber nicht. Sie erreicht ein Filter über **Anwenden**.
- Mail, deren `Message-ID` das Konto schon in einem anderen Ordner hat, bleibt unberührt. Sie wurde in den
  Posteingang verschoben oder kopiert, nicht zugestellt, und ein Filter soll sie nicht gleich wieder hinausnehmen.
  Kopien im Archiv und in den gesendeten Mails zählen dabei nicht: Gmail legt jede neue Mail auch in „All Mail“ ab,
  das für Torro der Archiv-Ordner ist, und Mail an sich selbst oder an eine Mailingliste hat eine Kopie in Sent. Diese
  Mail wurde trotzdem zugestellt und wird gefiltert. Nur eine Kopie in einem anderen Ordner (Spam, ein eigener Ordner)
  hält den Filter ab. Die Kehrseite: Holt der Mensch Mail aus dem Archiv zurück in den Posteingang, kann ein Filter sie
  erneut verschieben; dann hilft es, den Filter enger zu fassen oder auszuschalten.
- Beim Eintreffen kennt der Dienst Betreff und Adressen, meist aber noch nicht den Text. Einfache Wörter einer Suche
  prüft ein Filter bei neuer Mail deshalb nur dort. Vorschau und Anwenden durchsuchen auch den Text und können mehr
  treffen.
- Vorhandene Mail erreicht ein Filter nur über **Anwenden**, nach einer Vorschau der Treffer.

**Mehrere Filter** laufen in der Reihenfolge der Liste (ein neuer kommt ans Ende), und jeder passende wirkt. Als gelesen markiert, sobald einer es
verlangt; beim Verschieben und bei der Kategorie entscheidet der erste Filter, der eines setzt
(`FilterActions::of`). Ausgeführt wird in der Folge gelesen → Kategorie → Ordner.

**Folgen im Notification Hub:** Mail, die ein Filter als gelesen markiert oder verschoben hat, wird kein Eintrag,
denn der Abgleich legt nur ab, was noch ungelesen im Posteingang liegt. Setzt ein Filter nur eine Kategorie, kommt
die Mail mit dieser Kategorie in den Hub, vor den Sortierregeln und ohne das Modell zu fragen; eine von Hand
gesetzte Kategorie bleibt ([kategorien.md](kategorien.md#vorrang)). Eine Kategorie, die nicht mehr im Katalog steht,
überspringt der Filter. Scheitert eine Aktion, schreibt der Dienst Konto und Fehlercode ins Protokoll, nie Suche oder
Betreff. Die Mail bleibt dann in diesem Punkt, wie sie kam, und bekommt einen Eintrag, wenn sie noch ungelesen im
Posteingang liegt.

**Vorschau und Anwenden:**

- `mail.preview_filter { query, account? }` zählt, was eine Suche jetzt im Posteingang findet (im Konto bzw. in allen
  erreichbaren), und nennt die fünf neuesten Mails. Sie ändert nichts.
- `mail.apply_filter { id }` wendet einen gespeicherten Filter einmal auf den Posteingang an, auch einen
  ausgeschalteten. Je Aufruf nimmt er die 500 neuesten Treffer, die noch eine seiner Aktionen brauchen: ungelesene für
  „gelesen“, Mail ohne diese Kategorie (und ohne eine von Hand gesetzte) für „Kategorie“, jede für das Verschieben.
  Die Änderungen laufen in Stücken zu 100 über dieselben Wege wie `mail.set_flags`, `mail.move_messages` und
  `mail.delete_messages` (nur in den Papierkorb). Antwort `applied`, `failed` und `more`: Mit `more` bleibt noch
  etwas, ein weiterer Aufruf macht weiter. Setzt der Filter eine Kategorie, bekommt auch der Eintrag der Mail im
  Notification Hub sie, wie bei den Kategorie-Läufen; ein erledigter Eintrag und eine dort von Hand gewählte
  Kategorie bleiben.

**Rechte und Freigaben:** `mail.list_filters` und `mail.preview_filter` sind Read, alles andere ist Admin. Ein Filter
wirkt dauerhaft auf die Mail des Menschen, darum wartet bei Agenten jede Änderung und jedes Anwenden auf eine Freigabe,
nicht nur das Löschen.

- **Reichweite:** Ein Client sieht den Filter eines Kontos aus seinen `mail_accounts`, einen Filter für alle Konten nur
  mit Zugriff auf alle Mail-Konten. Jeder andere Filter gilt für ihn als nicht vorhanden (`not-found`).
- Einen Filter für alle Konten anlegen oder ändern ohne Zugriff auf alle Konten ist `forbidden`. Dasselbe gilt für
  eine Kategorie ohne Admin im Bereich `inbox` (sie wirkt wie eine Sortierregel), auch beim Anwenden eines Filters mit
  Kategorie. Der Dienst prüft das vor der Freigabe und noch einmal bei der Ausführung, ebenso Suche, Aktionen,
  Ordner und Kategorie (`error`, dann gibt es nichts freizugeben).
- **Rechte der Aktionen:** Admin auf `mail` allein reicht nicht. Weil der Dienst die Aktionen eines Filters selbst
  ausführt, braucht ein Client dazu die Rechte, die sie einzeln bräuchten: Schreiben für „gelesen“, Archiv und Ordner
  (wie `mail.set_flags` und `mail.move_messages`), Löschen für den Papierkorb (wie `mail.delete_messages`, auch für
  einen Ordner mit der Rolle Papierkorb). Sonst ist Anlegen, Ändern und Anwenden `forbidden`, vor der Freigabe und
  noch einmal bei der Ausführung.
- **Fehlende Kategorie oder fehlender Ordner:** Beim Ändern prüft der Dienst Kategorie und Ordner nur, wenn sie neu sind. Ein Filter,
  dessen Kategorie der Mensch gelöscht hat oder dessen Ordner es auf dem Server nicht mehr gibt, lässt sich so weiter
  ausschalten und ändern. Beim Eintreffen und Anwenden überspringt er die fehlende Kategorie; das Verschieben in den
  fehlenden Ordner scheitert, kommt ins Protokoll und zählt beim Anwenden als `failed`.
- Die Freigabe nennt den gespeicherten Filter mit der Adresse des Kontos und dem Namen des Ordners aus dem Speicher
  des Dienstes, nie die Worte des Clients außer der neuen Suche: beim Ändern alt und neu, beim Anwenden „up to n“. Sie
  ist an den gespeicherten Filter gebunden, beim Anwenden auch an die n Mails, die er da fand: Hat ihn jemand
  inzwischen geändert oder findet er bis zur Zustimmung andere Mail (ein ausgeschalteter Filter findet neue Mail
  weiter), endet sie mit `conflict` und ändert nichts.
- Ins Protokoll kommen Kennung und Konto, nie die Suche, ein Ordner oder eine Kategorie.
- Filter bleiben auf dem Rechner. Nur `mail.apply_filter` ändert Mail auf dem Server; die Tools der übrigen fünf
  Befehle sind entsprechend gekennzeichnet.

**Oberfläche:**

- **Einstellungen des Mail-Fensters › Filters** (nach Categorization, `--settings filters`): die Filter in ihrer
  Reihenfolge, je Zeile die Suche, darunter, was der Filter tut, mit Schalter; „New filter“ legt einen an.
- **Seite eines Filters:** die Suche, das Konto („All accounts“), was der Filter jetzt findet („In Inbox now“), die
  Aktionen „Move“ (Don’t move, Archive, Folder, Trash) mit dem Ordner des gewählten Kontos, „Mark as read“ und
  „Category“, der Schalter „Filter is on“ und bei einem gespeicherten Filter „Delete filter“ mit Rückfrage. „Save“
  steht in der Kopfleiste. Eine Kategorie, die es nicht mehr gibt, heißt „Unknown category“.
- **Speichern** wartet auf den Dienst („Saving…“): Erst wenn er den Filter gespeichert hat, schließt die Seite.
  Lehnt er ab, bleibt die Seite mit dem Eingegebenen offen, und die Meldung steht darüber.
- **Anwenden:** Findet ein gerade gespeicherter Filter Mail im Posteingang, fragt die Liste danach „Apply to the {n}
  mails in Inbox?“ mit „Not now“ und „Apply“; „Apply“ wendet den gespeicherten Filter an. Bleibt danach noch Mail
  übrig, macht „Apply to more“ weiter.
- **Schnell anlegen:** „Create filter…“ in der Gruppe More der Aktionsleiste einer Mail öffnet die Seite eines neuen
  Filters mit `from:<Absender>`. „Create filter from search…“ im Filter-Menü des Suchfelds (nur mit Text im Feld)
  macht die Suche zur Bedingung, mit dem Konto, wenn die Suche auf „This account“ steht, sonst für alle Konten.

**Was mit dem Ausblenden wegfiel:** die Ansicht „Hidden“, „Hide“ und „Show again“ im Mail-Fenster, die Regeln, die
Absender, Domains oder Merkmale von Mail ausblendeten, `inbox.hide_sender`, `inbox.set_mail_shown`, das Feld `hidden`
von `mail.search_messages` und die Wirkung `hide` von `inbox.mute_item` für Mail ([api.md](api.md#versionen)). Beim
Update entfernt die Migration auf Schema 17 von `inbox.sqlite` diese Regeln und ihre Markierungen. Früher
ausgeblendete Mail steht wieder in den Listen (unter „Unfiltered“). Nichts wird automatisch zu einem Filter. Ein
stummgeschaltetes Konto (`inbox.mute_source`), gemerkte Spuren und die Regeln anderer Quellen bleiben.

## Verhältnis zu TorroMail

| Weg | Folge |
|---|---|
| **A: getrennt weiterführen (entschieden 2026-10-12)** | TorroMail bleibt eigenständig mit eigenem MCP-Server. Die Suite hat Mail im Kern, `torroworkplace mcp` bietet eigene `mail_*`-Tools. Zwei Kontenspeicher, zwei Rechtemodelle, bewusst getrennt |
| B: Suite übernimmt | Mail kommt in den Kern, TorroMail wird eingestellt |
| C: TorroMail-Crates als Bibliothek | Synchroner Client, eigenes Rechtemodell, eigene Pfade: passt nicht zum Kern, bindet zwei Repos aneinander |

**Übernahme:** Findet die Steuerzentrale `~/.local/state/torromail/state.json` (macOS
`~/Library/Application Support/TorroMail`), bietet sie einmal an: „Use 3 accounts from
TorroMail?“. Übernommen werden Server, Benutzer, Anzeigename. Das Passwort holt der Kern nach
Zustimmung aus dem Schlüsselbund-Eintrag `TorroMail/<id>` und legt es unter `torroworkplace` ab.
Agenten werden **nicht** übernommen, sondern über den Assistenten (A.7) neu gekoppelt, damit
jeder mit Rechten nach dem neuen Modell startet. TorroMail behält seine Konten; nichts dort wird
geändert oder gelöscht.

## Reihenfolge

| Schritt | Inhalt | Braucht |
|---|---|---|
| **M1 Mehrere Konten** | `AccountKind::Mail`, Konten-Liste statt erstes Konto, PIM nach Konto geschlüsselt, Sammlungs-Kennungen mit Konto, Konto-Gruppen in Kalender und Aufgaben | – |
| **M7a Kalender und Aufgaben aus Mail-Konten** (vorgezogen 2026-10-11) | CalDAV-Konto mit Dienstsuche (RFC 6764, bekannte Anbieter), Assistent, Prüfung mit echten Anbietern, Übernahme aus TorroMail – ohne Mail-Kern. Aufgaben: [tasks/mail.md](tasks/mail.md) M7.1–M7.4 | M1 |
| **M2 Mail-Kern** | `torro-mail`, `torro-mail-imap`, Discovery, Ordner-Rollen, SQLite/FTS5, Sync mit IDLE | M1 |
| **M3 Befehle** | Requests, Katalog (Bereich Mail), MCP-Tools, `Rights` erweitert | M2 |
| **M4 Freigaben** | A.3 (Dienst) und A.8 (Oberfläche), Senden über Freigabe | M3 |
| **M5 Mail-App** | Breit/Mittel/Schmal/Mini, Schreibfenster, `--account` | M3 |
| **M6 Rechte von Agenten (API und MCP)** | A.6/A.7 mit Mail, TorroMail-Übernahme | M3, M4 |
| **M7 Dienste derselben Domain** | CardDAV beim Mail-Konto finden, Einladungen beantworten (CalDAV ist mit M7a vorgezogen) | M2, M7a |
| später | Sieve (Filter auf dem Server, Abwesenheit; Filter im Dienst gibt es seit #339, siehe „Filter“), Kontakte-App, OAuth Google/Microsoft (braucht registrierte Client-IDs, bei TorroMail auch noch offen), JMAP | |

Aus jedem Schritt werden Aufgaben in `docs/tasks/mail.md` (Präfix `M`), wenn das Konzept
freigegeben ist. M1 steht in [tasks/github.md](tasks/github.md), M7a schon in
[tasks/mail.md](tasks/mail.md).

## Entscheidungen (2026-10-12)

1. ~~Kontenleiste~~: keine Leiste (siehe oben).
2. **Zuschnitt:** Mail ist eine Quelle im Eingang (`torroinbox`), keine eigene App (Weg A in
   [eingang.md](eingang.md)). Ordner, Lesen und Schreiben kommen in den Eingang.
3. **TorroMail läuft parallel weiter.** Die Suite bekommt eine eigene, vollständig integrierte
   Mail-Anbindung (Kern, `mail_*`-Tools in `torroworkplace mcp`). TorroMail bleibt eigenständig mit
   eigenem MCP-Server; es gibt keinen gemeinsamen Code. Die Übernahme der Konten (M7.4) ist eine
   Abkürzung beim Einrichten, kein Abschied von TorroMail.
4. **Fehlende Ordner legt der Kern an**; ein Auswahlfeld nur bei zwei Kandidaten mit Inhalt.
5. **Senden** über den Schalter „Nach außen“ (never / with approval / free), Voreinstellung
   „with approval“ in allen Vorlagen. `Rights`, `Area` und `AccountKind` werden dafür erweitert
   (freigegeben).
6. **Lesen für Agenten** ja/nein je Konto (Stufe im Bereich Mail), ohne „nur Kopfzeilen“.
7. **Bibliotheken:** `mail-parser`, `mail-builder`, `lettre`, alles über rustls. **IMAP** ist eine eigene,
   kleine Schicht auf tokio und rustls (entschieden 2026-10-12 nach M2.3): `async-imap` kann `XLIST` und
   `NAMESPACE` nicht lesen, liefert Antwortcodes wie `[AUTHENTICATIONFAILED]` nur als Text und kann in IDLE
   abstürzen.
8. **Mail wird als HTML gezeigt** (entschieden 2026-10-14, ersetzt „HTML nur als Text“ aus M2.4/E4.2).
   Der Kern hebt den HTML-Teil bereinigt auf (ohne Skripte, Formulare, Frames, Objekte und
   Ereignis-Attribute; Stile bleiben) und gibt ihn mit `include_html` heraus. Gezeigt wurde er zuerst in einem
   **eigenen Fenster mit System-Webview** (M9.2, wry: WebKitGTK unter Linux, WKWebView unter macOS, WebView2
   unter Windows), weil Iced kein HTML zeichnet und sich unter Wayland keine Webview in ein Iced-Fenster
   einbetten lässt. *Geschichte:* Das Fenster gilt nicht mehr, Entscheidung 9 ersetzt es; mit M9.9 ist es
   samt WebKitGTK entfernt. Es bleibt: keine Skripte (Bereinigung im Kern), Links öffnen im Standard-Browser.
   **Externe Bilder werden immer geladen**, wie bei Gmail oder Apple Mail; dem Absender verrät das, dass
   und wann die Mail geöffnet wurde. Eingebettete Bilder (`cid:`) liefert der Kern über
   `mail.get_attachment`. Agenten bekommen weiter den Text und das HTML nur, wenn sie es ausdrücklich
   anfordern; es bleibt `untrusted`.
9. **HTML steht im Lesebereich, nicht in einem eigenen Fenster** (entschieden 2026-10-15, ändert das Fenster
   aus Entscheidung 8). Blitz zeichnet die Mail direkt in Iced (Weg 2 aus dem Versuch M9.4), überall gleich und
   ohne Systempakete; das eigene Fenster und damit WebKitGTK sind entfernt (M9.9). Damit Blitz typisches Mail-HTML richtig
   zeichnet, schreibt die Bereinigung im Kern alte Attribute (`cellpadding`, `border`, `<font>` …) in CSS um.
   Bilder lädt die Bibliothek über rustls (`reqwest`), nicht über `blitz-net` (OpenSSL). Zwei Einstellungen:
   **Ansicht** HTML (Voreinstellung) oder Text, im Lesebereich jederzeit umschaltbar; **Farben der Mail** wie
   die App (Voreinstellung, also System/Hell/Dunkel), hell oder dunkel – nur für den Inhalt der Mail, weil
   dunkel umgefärbte Newsletter oft schlechter aussehen und man das abschalten können muss.

Aufgaben: [tasks/mail.md](tasks/mail.md) (M2–M5, E4, M9).
