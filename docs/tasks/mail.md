# Aufgaben: Mail-Konten

Konzept: [mail.md](../mail.md). Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill
`torro-task`. Wer einen Request anfasst, lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`.

## Vorgezogen: Kalender und Aufgaben aus Mail-Konten (M7a)

Entschieden 2026-10-11: Kalender und Aufgaben eines Mail-Anbieters kommen **vor** dem Mail-Kern (M2).
IMAP selbst kennt keine Termine; viele Anbieter haben unter demselben Login CalDAV (mailbox.org,
Posteo, Fastmail, iCloud, Zoho, eigene Server mit Nextcloud/SOGo/Radicale). Unser CalDAV-Client
(`torro-pim-caldav`) ist schon allgemein; es fehlen ein Konto, das nicht Nextcloud ist, und die Suche
nach dem Dienst. Einladungen aus Mails (iMIP) und Kontakte (CardDAV) bleiben bei M7 hinter dem Mail-Kern.

Nicht dabei: Gmail (CalDAV nur mit OAuth und registrierter Client-ID) und Microsoft 365 (kein CalDAV,
nur Graph/EWS). Der Assistent sagt das freundlich, statt es zu versuchen.

```
M1.1 (tasks/github.md) ── M7.1 ─┬─ M7.2 ── M7.4
                                └─ M7.3
```

### M7.1 Kern: CalDAV-Konto mit Dienstsuche
- [x] erledigt (`docs/api-reference.md` gibt es noch nicht, A.9)
- **Modell:** Opus (Agent `nextcloud-expert` geeignet) · **Braucht:** M1.1 · **Dateien:**
  `crates/torro-core/src/account.rs`, `crates/torro-pim-caldav/src/{lib,client}.rs` und neu
  `discover.rs`, `crates/torroworkplace/src/{account,pim,core,mcp,clients}.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `docs/api.md`, `docs/api-reference.md`
- **Konto:** `AccountKind::CalDav` mit Server-URL (die gefundene DAV-Basis), Anmeldename und
  Anzeigename (Anbieter bzw. Domain, z. B. „mailbox.org“); Kennung `caldav:<login>@<host>` (nie mit
  `client-` beginnend, Anmeldename ohne `/`, weil M1.1 Sammlungs-Kennungen am ersten `/` trennt);
  Passwort bzw. App-Passwort im Schlüsselbund unter der Kennung wie bei Nextcloud. Erneutes Verbinden
  derselben Kennung („Change password…“) ersetzt nur die Zugangsdaten; ausgeblendete Sammlungen und
  Rechte-Auswahlen bleiben.
- **Neue Regel in `docs/api.md`, „Was nie nach außen geht“, Punkt 5:** Geheimnisse entgegennehmen oder
  übertragen (Anmeldung mit Passwort, Import aus einem fremden Schlüsselbund) nur mit dem App-Schlüssel,
  also nur aus der Oberfläche. Durchgesetzt in `Rights::allows` (auch Script-Schlüssel mit Admin bekommen
  `forbidden`) und im Test `names_follow_the_rules` (`kept_back`). Solche Requests landen nie in
  `approvals.json`.
- **Passwort als eigener Typ** `Secret` nach dem Vorbild von `Key` in `torro-ipc`: `Debug` zeigt `***`,
  Schema `string` mit `writeOnly`; ebenso in den Verbindungsdaten des CalDAV-Clients.
- **Dienstsuche** (`discover.rs`, ohne Anmeldung): aus einer E-Mail-Adresse bzw. Domain (nur die Domain
  geht hinaus, nie der lokale Teil) der Reihe nach
  1. DNS SRV `_caldavs._tcp.<domain>` (+ TXT `path=`), RFC 6764;
  2. `https://<domain>/.well-known/caldav` und `https://caldav.<domain>/.well-known/caldav` mit
     Weiterleitungen (höchstens 5, nur https);
  3. eine kleine Tabelle bekannter Anbieter (mailbox.org, Posteo, Fastmail, iCloud, Zoho; Gmail und
     Outlook/Hotmail/Microsoft-Domains als „nicht unterstützt“ mit Grund).
  Schutz gegen Missbrauch als Netz-Sonde: keine IP-Literale, Ziele (auch nach Weiterleitungen), die auf
  Loopback, private, Link-Local- oder ULA-Adressen auflösen, werden abgelehnt; nur https; kurze Timeouts,
  wenige Anfragen je Aufruf; die Antwort enthält nie Server-Inhalte oder Header; protokolliert wird nur der
  Befehl, nicht die Adresse.
  Ergebnis: Kandidaten-URL(s), „nicht gefunden“ oder „nicht unterstützt“ – alles normale Antworten. Mit Anmeldung prüft `CalDavClient` dann
  `current-user-principal` und meldet, ob es **Kalender** (VEVENT) und **Aufgaben** (VTODO) gibt.
- **Sync** wie Nextcloud über M1.1 (eigener Zustand je Konto); Nextcloud-Eigenes (Remote Wipe,
  Login Flow, Platzhalternamen) nur für Nextcloud-Konten. 401/403 → Problem „Password was refused“ am Konto.
- **API:**
  - `Request::DiscoverServices { address }` · `accounts.discover_services` · Zugriff Read – sucht ohne
    Anmeldung. Antwort `Response::Discovery { provider, urls, unsupported: Option<String> }`.
  - `Request::ConnectCalDav { address, server: Option<String>, login_name, password: Secret, calendars:
    bool, tasks: bool }` · `accounts.connect_caldav` · Zugriff Admin, nur App-Schlüssel (Regel oben) –
    meldet an, prüft, speichert das Passwort nur im Schlüsselbund, legt das Konto an, blendet nicht
    gewählte Dienste aus und gleicht ab. Antwort: das Konto (`AccountInfo`) und welche Dienste der Server
    hat (VEVENT/VTODO), ohne Passwort. Fehlercodes `auth`, `not-found` (unter der URL kein CalDAV),
    `unsupported`, `offline`.
  - Katalogbeschreibungen von `accounts.remove_account` und `accounts.sync` anpassen: App-Passwort-
    Widerruf und Remote Wipe gelten nur für Nextcloud-Konten.
- **MCP:** `accounts_discover_services` ja. `accounts_connect_caldav` **Never** – Punkt 5 der neuen
  Regel: Ein Agent nimmt nie ein Passwort entgegen und reicht es weiter.
- **Abnahme:** Tests ohne Netz: Suche gegen einen lokalen Fake-Resolver und `httpmock` (SRV, well-known
  mit Weiterleitung, Tabelle, nicht unterstützt), Anmelden mit falschem Passwort (`auth`), zwei Konten
  (Nextcloud + CalDAV) synchronisieren getrennt; IP-/Loopback-Ziel abgelehnt; Client-Schlüssel mit Admin
  → `forbidden` für `connect_caldav`; das Passwort erscheint weder in Antwort noch in `Debug`,
  Fehlermeldung, Protokoll oder `approvals.json`; API-Referenz neu erzeugt; `torro-api-review` ohne Befund.

### M7.2 Steuerzentrale: Mail-Anbieter im Assistenten
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** M7.1, M1.2 · **Dateien:** `crates/torroworkplace/src/ui/{wizard,accounts}.rs`
- **Oberfläche:** Im Assistenten neben „Nextcloud“ der Anbieter „Mail provider (calendar and tasks)“.
  Schritt 1: E-Mail-Adresse → `accounts.discover_services`; gefundener Anbieter mit Logo-freiem Namen,
  „Server“ nur aufklappbar unter „Advanced“. Bei „nicht unterstützt“ der Grund und nichts weiter.
  Schritt 2: Passwort-Feld mit Hinweis, wo es nötig ist: „Use an app password“ mit Link-Text für iCloud
  und Fastmail. Schritt 3: gefundene Dienste als Häkchen (Calendars, Tasks), vorausgewählt; „Mail“
  ausgegraut mit „Coming later“; die Wahl geht als `calendars`/`tasks` mit `accounts.connect_caldav`. Fehler `auth` am Passwort-Feld, erst nach dem ersten Versuch.
  Kontoseite: Art „CalDAV“, Server, Status, „Change password…“ (sendet `accounts.connect_caldav` erneut).
- **API/MCP:** nutzt M7.1; keine neuen Befehle.
- **Abnahme:** Sichtprüfung aller Schritte (Breit, Mittel, Schmal, hell/dunkel) mit `--demo`-Zuständen,
  Tests der Nachrichten.

### M7.3 Mit echten Anbietern prüfen
- [ ] erledigt
- **Modell:** Opus · **Braucht:** M7.1 · **Dateien:** `crates/torro-pim-caldav/tests/fixtures/*` (neu),
  `docs/mail.md` (Ergebnisse)
- Mit den Konten des Nutzers (nur nach Rückfrage, Zugangsdaten tippt der Nutzer selbst ein) und mit
  Radicale lokal: Kalender und Aufgaben lesen, anlegen, ändern, löschen; fremde Eigenschaften bleiben
  (P.1). Antworten der Server als Fixtures (ohne persönliche Daten) für Tests ablegen. Besonderheiten je
  Anbieter in `mail.md` festhalten (z. B. iCloud: Aufgaben nicht mehr per CalDAV seit iOS 13).
- **API/MCP:** keine.

### M7.4 Aus TorroMail übernehmen
- [ ] erledigt
- **Hinweis 2026-10-12:** TorroMail läuft parallel weiter; die Übernahme kopiert nur, TorroMail behält alles.
  Nach M2.7 übernimmt sie auch IMAP/SMTP (`imapHost`, `imapPort`, `imapSecurity`, `smtp*`, `username`, `name`,
  `email` aus `state.json`) und legt ein Mail-Konto (`accounts.connect_mail`) statt nur CalDAV an.
- **Modell:** Sonnet · **Braucht:** M7.2 (Mail: M5.1) · **Dateien:** `crates/torroworkplace/src/{account,core,mcp}.rs`,
  `crates/torroworkplace/src/ui/wizard.rs`, `crates/torro-ipc/src/{lib,api}.rs`, `docs/api-reference.md`
- **Oberfläche:** Gibt es TorroMail-Konten, bietet der Assistent oben „Use accounts from TorroMail?“ an:
  Liste mit Adresse und gefundenem CalDAV; übernommen werden Adresse, Server, Benutzer. Das Passwort holt
  der Kern **nach Zustimmung** aus dem Schlüsselbund-Eintrag `TorroMail/<id>` und legt es unter
  `torroworkplace` ab (siehe `mail.md`, Abschnitt TorroMail).
- **API:** `Request::TorroMailAccounts` · `accounts.list_torromail_accounts` · Read – Beschreibung:
  „Lists accounts of the separate TorroMail app (address, server, user), without passwords; they are not
  connected to the suite.“ `Request::ImportTorroMailAccount { id }` · `accounts.import_torromail_account` ·
  Admin, nur App-Schlüssel (Punkt 5 in `api.md`).
- **MCP:** `accounts_list_torromail_accounts` ja; Import **Never** – ein Agent könnte sonst fremde
  Zugangsdaten in die Suite holen. Die Zustimmung ist der Klick in der Oberfläche.
- **Abnahme:** Tests mit Fake-Schlüsselbund; Import nur mit App-Schlüssel (Client-Schlüssel → `forbidden`);
  `torro-api-review`.

## Mail im Kern und im Eingang (M2–M5, E4)

Entschieden 2026-10-12 (siehe [mail.md](../mail.md), „Entscheidungen“): Mail-Konten (IMAP/SMTP) kommen
vollständig in den Kern; gelesen und geschrieben wird im Eingang `torroinbox` (keine eigene Mail-App);
Agenten bekommen eigene `mail_*`-Tools in `torroworkplace mcp`. **TorroMail läuft parallel weiter**, es
gibt keinen gemeinsamen Code: Übernommen wird das Wissen (Dienstsuche, Ordnernamen, Versandregel,
Vertrags-Fixtures unter `../TorroMail/contracts/` und `../TorroMail/crates/*/tests/`). Bibliotheken:
`async-imap` (tokio), `mail-parser`, `mail-builder`, `lettre`, alles über rustls.

Ein Mail-Konto ist **eine Anmeldung mit Diensten**: Mail (IMAP + SMTP), dazu auf Wunsch Kalender und
Aufgaben über CalDAV derselben Anmeldung (M7.1). Ein Passwort im Schlüsselbund.

```
M7.1 ── M2.1 ─┬─ M2.2 ─────────────────┐
              ├─ M2.3 ── M2.5 ── M2.6 ─┼─ M2.7 ─┬─ M5.1 ── M7.4
              ├─ M2.4 ─────────────────┘        ├─ M5.2
              └─ M3.4                           ├─ M3.1 ─┬─ E4.2 ── E4.4
                                                ├─ M3.2 ─┴─ M3.3 ── E4.3
                                                └─ E4.1 ── E4.2
```

Übersetzungen: Laufen Aufgaben parallel, trägt nur die Koordination in `de.rs` ein; jede Aufgabe nennt
ihre Texte im Bericht.

### M2.1 Gerüst: Typen, Konto, Rechte, Requests, Katalog
- [x] erledigt
- **Modell:** Opus · **Braucht:** M7.1 · **Dateien:** neu `crates/torro-mail/` (Typen ohne Ein-/Ausgabe) und
  `crates/torro-mail-imap/` (nur Gerüst: `lib.rs` mit leeren Modulen `discover`, `imap`, `folders`, `smtp`,
  `mime`, damit M2.2–M2.4 parallel laufen), Wurzel-`Cargo.toml` (alle Abhängigkeiten für M2 schon hier),
  `crates/torro-core/src/account.rs`, `crates/torro-ipc/src/{lib,api}.rs`, neu `crates/torro-ipc/src/mail.rs`,
  `crates/torroworkplace/src/{core,mcp,clients,approvals}.rs`, `docs/api.md`, `docs/api-reference.md`
- **Typen (`torro-mail`):** `MailAccountConfig { address, sender_name, imap: Endpoint, smtp: Endpoint, caldav:
  Option<String> }`, `Endpoint { host, port, security: Tls|StartTls }`, `MailSettings { signature: Option<String>,
  notify: All|ForMeOnly|Off, services: { mail, calendars, tasks } }`, `Folder { id, account, name (angezeigt,
  UTF-7 dekodiert), path (wie vom Server), role: Option<Role>, unread, total }`, `Role` (Inbox, Drafts, Sent,
  Archive, Junk, Trash), `MessageSummary { id, account, folder, thread_id, from, to, subject, date, snippet,
  flags, has_attachments, untrusted: true }`, `Message` (dazu Text, HTML→Text, Anhänge als `AttachmentInfo`,
  `List-Id`/`Auto-Submitted`/`Precedence` als Merkmale, Einladung `text/calendar` als Kennzeichen),
  `Draft { id, account, to, cc, bcc, subject, body, in_reply_to, attachments }`, `Flag` (Seen, Flagged,
  Answered). Nachrichten-Kennung opak für Clients: der Kern vergibt sie in seinem Speicher (`m:<zufall>`) und
  löst sie dort auf, statt Pfade an Trennzeichen zu zerlegen. `Draft.attachments` ist eine Liste aus
  `MailPart { message, part }` (Anhang einer vorhandenen Mail, für Weiterleiten) oder `File { path }`.
- **Konto:** `AccountKind::Mail`, Kennung `mail:<adresse>` (Adressen mit `/` oder Steuerzeichen werden abgelehnt,
  wie in M7.1); `Account` bekommt `#[serde(default)] mail:
  Option<MailAccountConfig>` und `mail_settings: Option<MailSettings>` (verträglich, freigegeben 2026-10-12).
  Ein Mail-Konto mit `caldav` nimmt am PIM-Sync teil wie ein CalDAV-Konto aus M7.1 (dasselbe Passwort).
- **Rechte (freigegeben 2026-10-12):** `Area::Mail`; `Rights.mail_accounts: Selection` (fehlt → nichts, wie
  `inbox_sources`); `Rights.outbound: Outbound` (`never` / `with_approval` / `free`, Voreinstellung
  `with_approval`, auch in allen Vorlagen). `Rights::reaches_mail_account`. `outbound` und `confirm` wirken
  zusammen, die strengere Regel gilt (`free` mit `confirm: on_every_change` → `pending`). Hook-Clients
  (`Rights::hook()`/`for_kind`) bekommen `outbound: never`. Nicht freigegebene Konten →
  `not-found`, gefiltert vor dem Lesen.
- **Requests und Katalog** (Handler antworten bis zu ihrer Aufgabe mit `unsupported`):

  | Request | Befehl · Zugriff | MCP |
  |---|---|---|
  | `ConnectMail { address, sender_name, imap, smtp, login_name, password: Secret, calendars, tasks, caldav: Option }` | `accounts.connect_mail` · Admin, nur App-Schlüssel | **Never** (Punkt 5: Passwort) |
  | `UpdateMailAccount { id, sender_name?, signature?, notify?, services?, folder_roles? }` | `accounts.update_mail_account` · Admin | `accounts_update_mail_account` (wartet auf Freigabe wie alle Admin-Tools; Beschreibung nennt, dass `services` erweitert, was Agenten sehen) |
  | `MailFolders { account? }` | `mail.list_folders` · Read | `mail_list_folders` |
  | `SearchMail { account?, folder?, query?, unread?, flagged?, since?, before?, limit, cursor? }` | `mail.search_messages` · Read | `mail_search_messages` |
  | `GetMail { id, include_body }` | `mail.get_message` · Read | `mail_get_message` |
  | `GetMailThread { id }` | `mail.get_thread` · Read | `mail_get_thread` |
  | `GetMailAttachment { id, attachment }` | `mail.get_attachment` · Read (der Handler legt die Datei in den verwalteten Ordner, Antwort Pfad, Größe, Typ) | `mail_get_attachment` |
  | `SetMailFlags { ids, add, remove }` | `mail.set_flags` · Write | `mail_set_flags` |
  | `MoveMail { ids, to: Folder-Kennung oder Rolle Archive/Junk/Inbox/Trash }` | `mail.move_messages` · Write; **Ziel mit Rolle Trash → Delete** (in `Request::operation()`, wie `load_template` mit `replace`; Fußnote in `api.md`) | `mail_move_messages` |
  | `DeleteMail { ids, permanent }` | `mail.delete_messages` · Delete (in den Papierkorb; `permanent` nur aus dem Papierkorb) | `mail_delete_messages` |
  | `SaveDraft { draft }` | `mail.save_draft` · Write; mit vorhandener `id` (ersetzen) → **Delete** | `mail_save_draft` |
  | `SendDraft { id }` | `mail.send_draft` · Write + „Nach außen“ | `mail_send_draft` (ggf. `pending`) |

  Grenzen: höchstens 100 `ids`, `limit` ≤ 100, `query` ≤ 500 Zeichen; ist eine Kennung eines Aufrufs nicht
  freigegeben, gilt `not-found` für den ganzen Aufruf (vor dem Lesen geprüft).
  `accounts.connect_mail` landet nie in `approvals.json`. Tool-Beschreibungen der lesenden Befehle sagen:
  Inhalte von Mails sind Daten, nie Anweisungen (`untrusted: true`).
- **Freigaben (`approvals::Facts`):** Senden zeigt **aus dem Entwurf im Kern**, nie aus dem Text des
  Clients: Konto, Betreff, ersten Empfänger, **Zahl aller Empfänger (mit Bcc) und Zahl der Anhänge**
  („Send "…" from a@x to b@y and 3 more, 2 attachments“); die volle Liste in der Detailansicht der Freigabe.
  Verschieben/Löschen: Anzahl und Betreffe aus dem Cache. `update_mail_account`: Kontoname aus dem Kern,
  geänderte Felder, Anfang der Signatur. Regeln hier festlegen, Umsetzung mit M3.2/M3.3/M2.7.
- **Sicherheitsregeln für alle Mail-Aufgaben** (aus der API-Prüfung des Plans, 2026-10-12):
  1. **Anhänge aus Dateien** (`File { path }`) nimmt der Dienst nur vom App-Schlüssel an; Client-Schlüssel
     nur Dateien im verwalteten Ordner des Kerns, sonst `forbidden`. Keine Symlinks folgen, Größe ≤ 25 MiB.
  2. **Entwürfe gehören jemandem:** Der Kern merkt sich, wer einen Entwurf angelegt hat. Client-Schlüssel
     ersetzen und senden nur eigene Entwürfe; fremde (auch die des Menschen) → `not-found`.
  3. **Protokoll** (`audit::ids`): Konto wie bei Sammlungen, Nachrichten-Kennungen nur als Prüfsumme; nie
     Empfänger, Betreff, Suchbegriff, Dateiname, Ordnername.
  4. **Keine Geheimnisse in Logs:** Protokoll-Trace von `async-imap`/`lettre` nie einschalten; Konfigurationen
     tragen kein `Secret` in `Debug`.
  5. **Dateien:** `<data>/mail/` und `<cache>/mail-attachments/` 0700, alle Dateien darin (auch SQLite `-wal`/
     `-shm` und das SMTP-Journal) 0600.
- **Abnahme:** Tabellen-Test der Rechte deckt alle neuen Befehle ab; Client-Schlüssel mit Admin →
  `forbidden` für `connect_mail`; Vertragstest Tool ↔ Katalog; alte `clients.json` ohne neue Felder lädt
  (nichts frei, Senden „with approval“); `move_messages` nach Trash braucht Delete; `save_draft` mit `id`
  braucht Delete; Adresse mit `/` abgelehnt; API-Referenz neu erzeugt; `torro-api-review` ohne Befund.

### M2.2 Dienstsuche für Mail
- [ ] erledigt
- **Modell:** Opus · **Braucht:** M2.1 (Resolver-Trait und Schutzregeln aus M7.1) · **Dateien:**
  `crates/torro-mail-imap/src/discover.rs`, `crates/torro-mail-imap/tests/fixtures/*`, `crates/torro-ipc/src/lib.rs`
  (nur `Response::Discovery` um optionale Mail-Felder erweitern), `crates/torroworkplace/src/core.rs` (Handler)
- Reihenfolge wie TorroMail (`../TorroMail/crates/torromail-control/src/providers.rs`): Tabelle → MX (Google/
  Microsoft → „nicht unterstützt“, Grund: braucht OAuth) → `autoconfig.<domain>` → `/.well-known/autoconfig` →
  ISPDB (`autoconfig.thunderbird.net`) → SPF → Basis-Domain des MX gegen die Tabelle → SRV `_imaps._tcp`/
  `_submission._tcp` (RFC 6186). Kein TCP-Abtasten fremder Hosts. Gesamtbudget 8 s.
- Tabelle aus TorroMail übernehmen (GMX, web.de, mailbox.org, Posteo, Fastmail, iCloud, T-Online, Yahoo;
  Gmail/Outlook als „nicht unterstützt“ mit Grund und Hinweis auf App-Passwort bei Gmail) und um CalDAV-URLs
  ergänzen, wo bekannt. Schutz gegen Netz-Sonde wie M7.1 (keine IP-Literale, keine privaten Ziele, nur https),
  auch für Hosts aus MX, SPF, Autoconfig und ISPDB, **bevor** sie abgefragt oder zurückgegeben werden:
  Hostnamen syntaktisch geprüft und begrenzt (≤ 253 Zeichen), Ports nur 993/143/465/587, sonst verworfen.
  Höchstens zwei laufende Suchen je Client (`busy`). Die Katalogbeschreibung nennt, dass die Domain an
  `autoconfig.thunderbird.net` geht.
- **API:** `accounts.discover_services` (aus M7.1) liefert zusätzlich `imap`, `smtp` (Host, Port, Sicherheit),
  `login_hint` (Adresse oder lokaler Teil), `app_password: bool`. Neues optionales Feld, verträglich. **MCP:**
  `accounts_discover_services` wie gehabt. **Oberfläche:** M5.1.
- **Abnahme:** `../TorroMail/contracts/provider-discovery.json` als Fixture-Testfälle (Domains, MX, SPF,
  Autoconfig, App-Passwort), Fake-Resolver und `httpmock`, ohne Netz.

### M2.3 IMAP: Verbindung, Ordner, Rollen
- [ ] erledigt
- **Modell:** Opus · **Braucht:** M2.1 · **Dateien:** `crates/torro-mail-imap/src/{imap,folders}.rs`,
  `crates/torro-mail-imap/tests/*`
- Verbinden mit TLS oder STARTTLS (rustls, System- bzw. webpki-Wurzeln), `LOGIN`/`AUTHENTICATE PLAIN`,
  `CAPABILITY`, `NAMESPACE`, `LIST` mit `SPECIAL-USE`, sonst `XLIST`, sonst bekannte Namen (Liste aus TorroMail
  `common_names`, auch letzter Pfadteil). Modified UTF-7 nur für die Anzeige dekodieren, Pfade bleiben wie vom
  Server. **Fehlende Rolle Drafts/Sent anlegen** mit `NAMESPACE`-Präfix (mit `CREATE … (USE (\Sent))`, wenn der
  Server es kann) und abonnieren; zwei Kandidaten mit Inhalt → Rolle bleibt offen und wird gemeldet (M5.2 fragt).
  Anbieter, die Gesendetes selbst ablegen (Gmail, Outlook.com), bekommen keine Kopie.
- Abrufen: `UID SEARCH`, `UID FETCH` (Kopfzeilen gebündelt, `BODY.PEEK[]` für Inhalte), `UID STORE`, `UID MOVE`
  (sonst COPY + `\Deleted` + `UID EXPUNGE`), `APPEND`, `IDLE`, `CONDSTORE`/`HIGHESTMODSEQ` wenn vorhanden.
  Fehler als eigenes Enum (`Auth`, `Offline`, `Protocol`, `NotFound`), Passwort nie in Fehlern.
- **API/MCP/Oberfläche:** keine (Bibliothek für M2.6).
- **Abnahme:** Skriptierter IMAP-Server im Test (nach `../TorroMail/crates/torromail-core/tests/imap_contract.rs`):
  Anmelden ok/falsch, LIST mit SPECIAL-USE, XLIST, nur Namen (auch „Entw&APw-rfe“), fehlendes Sent wird angelegt,
  zwei Kandidaten → offen, MOVE ohne MOVE-Capability, UIDVALIDITY-Wechsel erkannt.

### M2.4 SMTP und MIME
- [ ] erledigt
- **Modell:** Opus · **Braucht:** M2.1 · **Dateien:** `crates/torro-mail-imap/src/{smtp,mime}.rs`,
  `crates/torro-mail-imap/tests/*`
- `mime`: Nachricht lesen mit `mail-parser` (Text bevorzugt, HTML nur zu Text umgewandelt, nie Fremdinhalte
  laden; Anhänge, `text/calendar`, Listen-Merkmale), Nachricht bauen mit `mail-builder` (Antwort/Weiterleiten mit
  `In-Reply-To`/`References`, Signatur, Anhänge, `Message-ID` vom Kern).
- `smtp` mit `lettre` (tokio, rustls; 465 TLS, 587 STARTTLS). **Versandregel aus TorroMail:** Journal
  (`<data>/mail/send-journal/`, 0600, nur Kennung und Status, nie die Nachricht) vor dem
  Versuch (fsync) und nach `DATA`; Ergebnis `accepted` / `not_accepted` / `unknown`; `unknown` wird **nie**
  automatisch erneut gesendet. Danach Kopie per `APPEND` in Gesendet (mit `\Seen`), Kopie-Fehler → später nur die
  Kopie erneut versuchen, nie die Mail.
- **API/MCP/Oberfläche:** keine (Bibliothek für M3.3).
- **Abnahme:** Skriptierter SMTP-Server (nach `../TorroMail/crates/torromail-mcp/tests/support/smtp.rs`): Annahme,
  Ablehnung, Abbruch nach `DATA` → `unknown` und kein zweiter Versuch; MIME-Rundreise mit Umlauten, Anhang,
  Antwort-Kopfzeilen; HTML-Mail mit Tracking-Bild wird Text ohne Abruf; nach falscher Anmeldung steht das
  Passwort in keinem Log und keinem Fehler.

### M2.5 Mail-Speicher im Kern
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** M2.1 · **Dateien:** neu `crates/torroworkplace/src/mail/store.rs` (und
  `mail/mod.rs`)
- SQLite (WAL, `rusqlite` gibt es schon) je Konto unter `<data>/mail/<konto-hash>.sqlite`, Dateien 0600:
  Ordner (Pfad, UIDVALIDITY, HIGHESTMODSEQ), Nachrichten (Kopf, Flags, Thread, Merkmale), Inhalte (nur Text),
  FTS5 über Betreff, Absender, Empfänger, Text, Dateinamen. Zeilen gelten nur unter ihrer UIDVALIDITY.
  **Der Kern entscheidet:** Kopfzeilen aller Ordner, Inhalte der letzten 90 Tage, Anhänge nur bei Bedarf (im
  verwalteten Ordner `<cache>/mail-attachments/`, nach 24 h gelöscht). Konto entfernen löscht seine Datei.
- **API/MCP/Oberfläche:** keine.
- **Abnahme:** Tests in Temp-Verzeichnis: Einfügen, Suche mit FTS5 (Umlaute), UIDVALIDITY-Wechsel verwirft den
  Ordner, Aufräumen alter Anhänge, Entfernen des Kontos; Rechte 0700/0600 auch für `-wal`/`-shm` (Unix).

### M2.6 Mail-Sync im Kern
- [ ] erledigt
- **Modell:** Opus · **Braucht:** M2.3, M2.5 · **Dateien:** neu `crates/torroworkplace/src/mail/{service,sync}.rs`,
  `crates/torroworkplace/src/{daemon,core,account}.rs`
- `MailService` je Konto (wie `PimService` nach Konto geschlüsselt): eine IDLE-Verbindung auf dem Posteingang
  (Push), die übrigen Ordner alle 5 Minuten mit CONDSTORE bzw. UID-Vergleich, eine zweite Verbindung für Befehle.
  Zurückhaltend neu verbinden (Backoff bis 15 min), offline erkennen. Ein Konto mit Fehler lässt die anderen
  unberührt. 401/`AUTHENTICATIONFAILED` → Problem „Password was refused“ am Konto (wie M7.1), sichtbar in
  `accounts.list_accounts` und als Suite-Eintrag im Eingang (E2.1).
- Neue Nachrichten melden ein Ereignis (für E4.1 und Änderungsmeldungen an die Apps).
- `accounts.sync` gleicht auch Mail ab. **API:** sonst keine neuen Befehle. **MCP:** keine neuen.
- **Abnahme:** Fake-IMAP im Test: Erstabgleich, neue Mail über IDLE, Flag-Änderung von außen, Verbindungsabbruch
  mit Wiederverbinden, zwei Konten getrennt, falsches Passwort → Problem am Konto.

### M2.7 Mail-Konto verbinden
- [ ] erledigt
- **Modell:** Opus · **Braucht:** M2.2, M2.4, M2.6 · **Dateien:** `crates/torroworkplace/src/{core,account,pim}.rs`,
  `crates/torroworkplace/src/mail/service.rs`
- Handler `accounts.connect_mail`: IMAP-Anmeldung prüfen, SMTP-Anmeldung prüfen (ohne zu senden), Passwort nur in
  den Schlüsselbund, Konto anlegen bzw. bei derselben Kennung nur Zugangsdaten und Server ersetzen (Einstellungen,
  ausgeblendete Sammlungen, Rechte bleiben). Mit `calendars`/`tasks` und gefundenem CalDAV nimmt das Konto am
  PIM-Sync teil. Antwort: `AccountInfo`, gefundene Dienste, offene Ordner-Rolle. Fehlercodes `auth` (mit welchem
  Dienst: IMAP/SMTP/CalDAV), `offline`, `unsupported`, `not-found`.
  `accounts.update_mail_account` ändert nur Einstellungen (Absendername, Signatur, Benachrichtigungen, Dienste);
  Server und Passwort nur über `connect_mail` (ein Agent darf das Passwort nicht auf einen anderen Server lenken).
  `accounts.remove_account` entfernt Mail-Speicher und Geheimnis.
  **Gibt es schon ein CalDAV-Konto (M7.1) derselben Anmeldung** (gleicher Anmeldename und CalDAV-Host), wird es
  in das Mail-Konto übernommen: Ausgeblendete Sammlungen und Rechte-Auswahlen werden auf die neuen Kennungen
  umgeschrieben (atomar), das alte Konto entfällt; es laufen nie zwei Abgleiche derselben Sammlungen.
- **MCP:** `accounts_update_mail_account` ja (Admin, Freigabe); `connect_mail` Never.
- **Abnahme:** Fake-IMAP/SMTP: Verbinden ok, falsches Passwort je Dienst (`auth`), erneutes Verbinden behält
  Einstellungen, `update_mail_account` kann keinen Server ändern, vorhandenes CalDAV-Konto wird
  übernommen (Auswahlen umgeschrieben), Entfernen löscht Speicher und Geheimnis; das
  Passwort erscheint nirgends (Antwort, `Debug`, Fehler, Protokoll, `approvals.json`).

### M3.1 Mail lesen: Befehle und MCP
- [ ] erledigt
- **Modell:** Opus · **Braucht:** M2.7 · **Dateien:** `crates/torroworkplace/src/{core,mcp}.rs`,
  `crates/torroworkplace/src/mail/{service,read}.rs`
- Handler für `mail.list_folders`, `mail.search_messages` (Cache mit FTS5; liegt der Zeitraum außerhalb des Caches,
  zusätzlich `UID SEARCH` am Server; Ergebnis mit `cursor` seitenweise, `source: cache|live`),
  `mail.get_message` (Inhalt fehlt → vom Server holen, `BODY.PEEK`, also ohne „gelesen“ zu setzen),
  `mail.get_thread`, `mail.get_attachment` (verwalteter Ordner, höchstens 50 MiB, Dateiname bereinigt).
- Rechte: Stufe Read im Bereich Mail, nur freigegebene Konten (`not-found` sonst, vor dem Lesen gefiltert).
  Alle Antworten mit Mail-Inhalt `untrusted: true`. Protokoll ohne Suchbegriffe und Inhalte.
- **Oberfläche:** E4.2. **MCP:** die fünf `mail_*`-Lesetools.
- **Abnahme:** Handler-Tests mit Fake-Konto; Client ohne Freigabe des Kontos → `not-found`; Anhangsname mit
  `../` landet im verwalteten Ordner; `get_message` setzt kein `\Seen`; Protokoll ohne Suchbegriff, Ordnername und Dateiname.

### M3.2 Mail ändern: Flags, Verschieben, Löschen, Entwürfe
- [ ] erledigt
- **Modell:** Opus · **Braucht:** M2.7 · **Dateien:** `crates/torroworkplace/src/{core,mcp,approvals}.rs`,
  `crates/torroworkplace/src/mail/{service,write}.rs`
- `mail.set_flags`, `mail.move_messages` (Rolle → Ordner des Kontos; nur innerhalb eines Kontos, sonst
  `unsupported`), `mail.delete_messages` (in den Papierkorb; `permanent` nur für Nachrichten im Papierkorb,
  Zugriff Delete), `mail.save_draft` (in den Entwürfe-Ordner per `APPEND`, alten Entwurf ersetzen; Kennung und
  Urheber des Entwurfs im Kern gemerkt; Sicherheitsregeln 1 und 2 aus M2.1). Optimistisch im Cache, bei Fehler zurück.
- Freigaben: `approvals::Facts` nach M2.1 (Anzahl, Betreffe aus dem Cache).
- **Oberfläche:** E4.2/E4.3. **MCP:** `mail_set_flags`, `mail_move_messages`, `mail_delete_messages`, `mail_save_draft`.
- **Abnahme:** Handler-Tests mit Fake-IMAP; Löschen mit `confirm: on_delete` → `pending`, Zusammenfassung aus
  dem Cache; Verschieben über Konten → `unsupported`; Verschieben nach Trash mit nur Write → `forbidden`;
  Client-Entwurf mit Pfad außerhalb des verwalteten Ordners → `forbidden`; Client ersetzt Entwurf des Menschen →
  `not-found`.

### M3.3 Senden mit Freigabe
- [ ] erledigt
- **Modell:** Opus · **Braucht:** M2.4, M3.2 · **Dateien:** `crates/torroworkplace/src/{core,mcp,approvals}.rs`,
  `crates/torroworkplace/src/mail/{service,send}.rs`
- `mail.send_draft`: baut die Nachricht aus dem Entwurf im Kern, sendet nach der Versandregel (M2.4), legt die
  Kopie ab, entfernt den Entwurf. App-Schlüssel (der Mensch in der Oberfläche) sendet direkt. Client: `outbound`
  `never` → `forbidden`, `with_approval` → `pending` (Freigabe zeigt Konto, Empfänger, Betreff, Textanfang aus dem
  Entwurf im Kern; **kein Code an den Agenten**), `free` → sendet. Ändert sich der Entwurf nach der Anfrage, ist
  die Freigabe ungültig (Fingerabdruck über die fertigen MIME-Bytes). Client-Schlüssel senden nur eigene
  Entwürfe (Regel 2). Ausstehende Kopien werden beim Start und jede Minute nachgeholt.
- **Oberfläche:** E4.3 (Senden), Freigabe in Steuerzentrale/Benachrichtigung (gibt es, A.8). **MCP:** `mail_send_draft`.
- **Abnahme:** Fake-SMTP: Senden mit App-Schlüssel; Client mit `with_approval` → `pending`, nach Freigabe gesendet;
  geänderter Entwurf → Freigabe verfällt; `never` → `forbidden`; Client sendet Entwurf des Menschen → `not-found`;
  Zusammenfassung nennt Zahl der Empfänger mit Bcc; `unknown` wird nicht wiederholt; Kopie-Fehler → nur Kopie wird
  nachgeholt; Empfänger stehen nicht im Protokoll.

### M3.4 Agents & API: Mail-Rechte
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** M2.1 · **Dateien:** `crates/torroworkplace/src/ui/{agents,agent_wizard}.rs`
- **Oberfläche:** Auf der Karte je Agent der Bereich „Mail“ (none/read/write/full) mit Auswahl der Konten wie bei
  Kalendern; darunter „Send on my behalf“: Never / Ask me each time / Always. Bei „Always“ ein Hinweis: Mails können
  Anweisungen an Agenten enthalten; damit würde ohne Rückfrage gesendet. Vorlagen setzen Mail mit, Senden
  bleibt „Ask me each time“. **API/MCP:** nutzt `clients.update_client` (Never, Punkt 2).
- **Abnahme:** Sichtprüfung (Breit, Schmal, hell/dunkel), Tests der Nachrichten.

### E4.1 Mail als Quelle im Eingang
- [ ] erledigt
- **Modell:** Opus · **Braucht:** M2.6 · **Dateien:** `crates/torro-inbox/src/{item,rules,source}.rs`,
  `crates/torroworkplace/src/{inbox,inbox_notify}.rs`, `crates/torroworkplace/src/mail/service.rs`
- `SourceKind::Mail`, Name = Konto-Kennung (Schlüssel `mail:<adresse>`, in `inbox_sources` auswählbar). Neue,
  ungelesene Mail im Posteingang → Eintrag: Thread als `thread_key` (Zähler statt zweitem Eintrag), Spur nach den
  Regeln aus [eingang.md](../eingang.md) (`List-Id`, `List-Unsubscribe`, `Precedence: bulk`, `Auto-Submitted` →
  Info; sonst Für mich; gemerkte Regeln je Absender gelten), Titel Betreff, `from` Absender, `untrusted: true`,
  `link` auf die Nachricht. Einstellung `notify` des Kontos gilt (Off → keine Einträge, ForMeOnly → nur Für mich).
- **Erledigt wirkt zurück:** erledigt → als gelesen markieren (nicht archivieren); woanders gelesen oder
  verschoben → Eintrag erledigt sich selbst. Beim ersten Abgleich eines Kontos nur Mails der letzten 3 Tage.
- **API/MCP:** keine neuen Befehle. Regel wie Fußnote ⁴ (`create_task_from_item`), als Fußnote in `api.md`:
  Mail-Einträge sehen braucht `inbox_sources` **und** Mail Read **und** das freigegebene Konto; die Rückwirkung
  von „erledigt“ (`\Seen` am Server) braucht Mail Write, sonst `forbidden`. Geprüft vor der Freigabe und bei
  ihrer Ausführung. **Oberfläche:** erscheint in `torroinbox` wie jede Quelle.
- **Abnahme:** Tests: neue Mail → Eintrag in der richtigen Spur, zweite Mail im Thread → Zähler, woanders gelesen →
  erledigt, erledigt → `\Seen` am Server, Newsletter → Info, erster Abgleich ohne Flut;
  Client mit Inbox-Rechten, aber ohne Mail Read sieht keine Mail-Einträge; ohne Mail Write → `set_item_done` an
  Mail-Eintrag `forbidden`.

### E4.2 Eingang: Ordner und ganze Mail
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** E4.1, M3.1, M3.2 · **Dateien:** `crates/torroinbox/src/{app,list,detail,keys}.rs`,
  neu `crates/torroinbox/src/mail.rs`, `crates/torro-ipc/src/inbox.rs` (Client-Aufrufe), Demo-Daten in
  `crates/torro-inbox/src/demo.rs`
- **Oberfläche:** Seitenleiste: Gruppe „Mail“ ▸ je Konto (ab zwei Konten) ▸ Ordner mit Ungelesen-Zahl
  (`mail.list_folders`). Klick zeigt den Ordner in derselben Liste (`mail.search_messages`), Suche oben.
  Paneel zeigt die ganze Mail (`mail.get_message`): Kopf, Text, Anhänge (öffnen über `mail.get_attachment`),
  Knöpfe Reply, Reply all, Forward, Archive, Delete, Flag, Mark unread. Tasten `r`, `a` (alle), `f`, `e` (Archiv
  im Ordner, erledigt im Eingang), `#` löschen. `--source mail` zeigt nur Mail. Externe Bilder werden nie geladen.
  Größenklassen nach `torro_gui::shell` (Breit/Mittel/Schmal), Skill `torro-design`.
- **API/MCP:** nutzt M3.1/M3.2.
- **Abnahme:** Sichtprüfung mit `--demo` (zwei Mail-Konten), Breit/Mittel/Schmal, hell/dunkel; Tests der Nachrichten.

### E4.3 Schreibfenster
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** M3.3, E4.2 · **Dateien:** neu `crates/torroinbox/src/compose.rs`,
  `crates/torroinbox/src/{main,app}.rs`
- **Oberfläche:** Eigenes Fenster (`torroinbox --compose [--reply <id>|--forward <id>]`, Fensterklasse
  `torro-compose`, kachelt sich neben die Liste). Felder Von (Konto), An, Cc/Bcc (aufklappbar), Betreff, Text mit
  Signatur, Anhänge (Dateiauswahl). Antwort zitiert. Speichert automatisch als Entwurf (`mail.save_draft`, alle
  paar Sekunden nach Änderung), „Send“ (`mail.send_draft`), Fehler bleiben im Fenster, nichts geht verloren.
- **API/MCP:** nutzt `mail.save_draft`, `mail.send_draft`.
- **Abnahme:** Sichtprüfung (Breit, Schmal, hell/dunkel), Tests der Nachrichten; Senden-Fehler lässt den Entwurf stehen.

### E4.4 Mini, Leiste, Benachrichtigung für Mail
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** E4.2 · **Dateien:** `crates/torroinbox/src/glance.rs`,
  `crates/torroworkplace/src/inbox_notify.rs`
- **Oberfläche:** Mini/Leiste zeigen ungelesene Mails je Konto und die drei neuesten Absender; Desktop-Benachrichtigung
  für neue Mail in „Für mich“ mit „Archive“ und „Mark read“; Ruhezeiten (E3.4) gelten.
- **API/MCP:** nutzt vorhandene Befehle.
- **Abnahme:** Sichtprüfung Mini/Leiste, Tests der Nachrichten.

### M5.1 Steuerzentrale: Mail-Konto im Assistenten
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** M2.7, M7.2 · **Dateien:** `crates/torroworkplace/src/ui/{wizard,accounts}.rs`
- **Oberfläche:** Der Anbieter aus M7.2 heißt „Mail account“. Schritt 1 Adresse → `accounts.discover_services`;
  gefundener Anbieter; „Advanced“ klappt IMAP/SMTP (Host, Port, Sicherheit), Anmeldename und CalDAV-URL auf
  (Pflicht, wenn nichts gefunden wurde). Schritt 2 Passwort (Hinweis App-Passwort bei iCloud, Fastmail, Yahoo,
  Gmail). Schritt 3 Dienste als Häkchen: Mail, Calendars, Tasks (vorausgewählt, was gefunden wurde), Absendername.
  „Connect“ → `accounts.connect_mail`; `auth` zeigt, welcher Dienst das Passwort abgelehnt hat.
- **API/MCP:** nutzt M2.2/M2.7.
- **Abnahme:** Sichtprüfung aller Schritte mit `--demo`-Zuständen (gefunden, nicht gefunden, nicht unterstützt,
  Passwort falsch), Breit/Mittel/Schmal, hell/dunkel; Tests der Nachrichten.

### M5.2 Steuerzentrale: Kontoseite Mail
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** M2.7 · **Dateien:** `crates/torroworkplace/src/ui/accounts.rs`
- **Oberfläche** (Einstellungen nach dem Vorbild TorroMail, ohne dessen Rechte und Cache-Stufen): Status je Dienst
  (Mail, Calendars, Tasks) mit letztem Abgleich; Absendername; Signatur (mehrzeilig); „Show new mail in Inbox“:
  All / Only „For me“ / Off; Dienste an/aus; „Change password…“ und „Server settings…“ (beide über
  `accounts.connect_mail`); nur bei offener Rolle einmal die Frage „Which folder holds sent mail?“ (`folder_roles` in
  `accounts.update_mail_account`); „Sync now“;
  „Remove account…“. Gespeichert über `accounts.update_mail_account`.
- **API/MCP:** nutzt M2.7, `accounts.sync`, `accounts.remove_account`.
- **Abnahme:** Sichtprüfung (Breit, Schmal, hell/dunkel), Tests der Nachrichten.

### Danach
- **M7.5 Einladungen (iMIP):** `calendar.respond_invitation` · Write + „Nach außen“ · MCP
  `calendar_respond_invitation`; im Paneel „Accept / Maybe / Decline“, Termin in den Standardkalender, Antwort per Mail.
- **M7.6 Kontakte (CardDAV)**, Empfänger-Vorschläge im Schreibfenster.
- Sieve (Filter, Abwesenheit), OAuth für Google/Microsoft (braucht registrierte Client-IDs), JMAP.
