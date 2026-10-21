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
Cache-Auftrag ergänzt fehlende Originale auch für bereits zwischengespeicherte Texte.
Das Originaldepot überlebt das Löschen von Textcache und veralteten Headern, benötigt
entsprechend dauerhaft Speicherplatz und wird erst mit dem Konto entfernt. Nur diese
Originale sind Quelle eines lokalen ZIP-Backups; fehlende Originale werden ausgewiesen.

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
| Suchen / Liste | Mail-Liste, Suche | `mail.search_messages` · Read | `mail_search_messages` |
| Stand des Suchindex (#136) | Suchfeld: Hinweis „12 400 von 18 000 indiziert“ (Schritt 5 von #136) | `mail.index_status` · Read | `mail_index_status` |
| Bestand und lokaler Cache (#263) | Mail-Einstellungen › Accounts und Storage & index | `mail.account_stats`, `mail.get_cache_policy`, `mail.preview_cache` · Read; `mail.set_cache_policy` · Admin | entsprechende `mail_*`-Tools |
| Cache und Index warten (#263) | Mail-Einstellungen › Storage & index | `mail.start_cache_job`, `mail.control_cache_job` · Write; `mail.cache_job_status` · Read | entsprechende `mail_*`-Tools |
| Nachricht lesen | Lesebereich | `mail.get_message` · Read | `mail_get_message` |
| Unterhaltung | Lesebereich | `mail.get_thread` · Read | `mail_get_thread` |
| Anhang holen | Lesebereich | `mail.get_attachment` · Read | `mail_get_attachment` |
| Mail als HTML (M9) | Lesebereich der Inbox (Schalter HTML/Text, Taste `v`) | `mail.get_message` mit `include_html` · Read | `mail_get_message` mit `include_html` |
| Flags setzen | Liste, Lesebereich | `mail.set_flags` · Write | `mail_set_flags` |
| Entwurf speichern | Schreibfenster | `mail.save_draft` · Write | `mail_save_draft` |
| Verschieben / Archiv | Liste, Lesebereich | `mail.move_messages` · Write (nach Trash: Delete) | `mail_move_messages` |
| Löschen | Liste, Lesebereich | `mail.delete_messages` · Delete | `mail_delete_messages` |
| Entwurf senden | Schreibfenster | `mail.send_draft` · Write + Senden-Schalter | `mail_send_draft` (antwortet ggf. `pending`) |
| Einladung beantworten (E8.3b) | Leser, Karte, Smart view „Invitations“ | `mail.respond_to_invitation` · Write + Nach außen | `mail_respond_to_invitation` (antwortet ggf. `pending`) |
| Freigabe entscheiden | Steuerzentrale › Approvals, Benachrichtigung | `approvals.decide` · Admin | **Never** (Liste Punkt 3) |
| Agenten-Rechte | Steuerzentrale › MCP (Agenten), › API (Skripte) | `clients.*` · Admin | **Never** (Liste Punkt 2) |

Statt TorroMails Paar `prepare_*` / `confirm_action` gibt es einen Request; ob er sofort läuft
oder auf eine Freigabe wartet, entscheidet der Dienst nach den Rechten des Agenten.

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
| später | Sieve (Filter, Abwesenheit), Kontakte-App, OAuth Google/Microsoft (braucht registrierte Client-IDs, bei TorroMail auch noch offen), JMAP | |

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
