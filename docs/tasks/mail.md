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
- [x] erledigt (nur Mail: kein CalDAV beim Import, Kalender auf der Kontoseite mit M3.5; `docs/api-reference.md` gibt es noch nicht, A.9)
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
`mail-parser`, `mail-builder`, `lettre`, alles über rustls; IMAP als eigene Schicht (M2.3, siehe
[mail.md](../mail.md), Entscheidung 7).

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
  4. **Keine Geheimnisse in Logs:** Protokoll-Trace der IMAP-Schicht und von `lettre` nie einschalten; Konfigurationen
     tragen kein `Secret` in `Debug`.
  5. **Dateien:** `<data>/mail/` und `<cache>/mail-attachments/` 0700, alle Dateien darin (auch SQLite `-wal`/
     `-shm` und das SMTP-Journal) 0600.
- **Abnahme:** Tabellen-Test der Rechte deckt alle neuen Befehle ab; Client-Schlüssel mit Admin →
  `forbidden` für `connect_mail`; Vertragstest Tool ↔ Katalog; alte `clients.json` ohne neue Felder lädt
  (nichts frei, Senden „with approval“); `move_messages` nach Trash braucht Delete; `save_draft` mit `id`
  braucht Delete; Adresse mit `/` abgelehnt; API-Referenz neu erzeugt; `torro-api-review` ohne Befund.

### M2.2 Dienstsuche für Mail
- [x] erledigt
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
- [x] erledigt
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
- [x] erledigt
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
- [x] erledigt
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
- [x] erledigt
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

### M2.7b Konten absichern (aus den Berichten von M2.6/M2.7)
- [x] erledigt (zusätzlich angefasst: `torro-mail-imap/src/smtp.rs` additiv `SmtpSubmitter::pinned_to`,
  `torroworkplace/src/pim.rs` für gefundene CalDAV-Adressen und den Kalenderteil von Mail-Konten,
  Beschreibung von `accounts.remove_account` in `torro-ipc/src/api.rs` und `docs/api.md`; offen: der Sync
  reiner CalDAV-Konten (M7.1) prüft keine öffentliche Adresse, weil ein eingetippter Server im eigenen Netz
  stehen darf)
- **Modell:** Opus · **Braucht:** M2.7 · **Dateien:** `crates/torroworkplace/src/{core,clients,account}.rs`,
  `crates/torroworkplace/src/mail/service.rs`, `crates/torro-mail-imap/src/imap.rs` (nur additiv),
  `crates/torro-pim-caldav/src/client.rs` (nur additiv)
- **Rechte vergessen beim Entfernen:** `accounts.remove_account` nimmt die Kennung des Kontos (und seiner
  Sammlungen) aus allen Auswahlen in `clients.json` (`mail_accounts`, `calendars`, `task_lists`, `inbox_sources`),
  atomar. Sonst bekommt ein Agent beim erneuten Verbinden derselben Adresse seinen Zugang still zurück.
- **Geprüfte Adresse festhalten (DNS-Rebinding):** Prüfung „öffentlicher Host“ und Verbindung lösen heute getrennt
  auf (Dienstsuche, `connect_caldav`, `connect_mail`, Mail-Sync). Verbinden mit der geprüften IP, Hostname nur für
  SNI und Zertifikat (`Session::connect_to(addr, &Endpoint, …)`; für reqwest `resolve`).
- **Ordner-Rollen Ende-zu-Ende:** Fake-IMAP mit STATUS und zwei Kandidaten; `connect_mail` meldet die offene Rolle,
  `update_mail_account { folder_roles }` setzt sie.
- **Wettlauf** `update_mail_account` (Kalender an) gegen `connect_caldav`: unter einer Sperre entscheiden.
- **Oberfläche/API/MCP:** keine neuen Befehle.
- **Abnahme:** Tests für alle vier Punkte; `torro-api-review` ohne Befund.

### M2.8 Mail-Konto lesen (Lücke aus M5.2)
- [x] erledigt
- **Modell:** Opus · **Braucht:** M2.7 · **Dateien:** `crates/torro-ipc/src/{lib,api,pim}.rs`,
  `crates/torroworkplace/src/{core,account,pim}.rs`, `crates/torroworkplace/src/mail/{service,sync}.rs`,
  `crates/torroworkplace/src/ui/{accounts,agents,glance,launch,overview,wizard}.rs` (nur `mail: None` in
  `AccountInfo`-Literalen), `docs/api.md`
- Die Kontoseite (M5.2) kann heute nichts von einem Mail-Konto lesen. `AccountInfo` bekommt das optionale Feld
  `mail: Option<MailView>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`, verträglich):
  `MailView { address, sender_name, imap: Endpoint, smtp: Endpoint, caldav: Option<String>, settings: MailSettings,
  open_roles: Vec<OpenRoleView { role, candidates: Vec<FolderChoice { name, path }> }>, mail: ServiceState,
  calendars: Option<ServiceState> }` mit `ServiceState { last_sync, problem }` je Dienst. Nie ein Geheimnis.
- **Erneut verbinden ändert nur, was die Anfrage nennt:** `ConnectMail` mit leerem `sender_name` behält den
  gespeicherten; Doc-Kommentar und Test. (Sonst löscht „Change password…“ den Absendernamen.)
- **API:** `accounts.list_accounts` (Read) liefert das Feld nur für Mail-Konten, die der Client erreicht;
  Beschreibung um einen Satz ergänzen. **MCP:** dasselbe Tool. **Oberfläche:** M5.2.
- **Abnahme:** Test: Mail-Konto in `list_accounts` mit Einstellungen, Servern, offener Rolle samt Kandidaten,
  Zustand je Dienst; Client ohne Freigabe sieht es nicht; kein Passwort in der Antwort; erneutes Verbinden mit
  leerem Absendernamen behält ihn; `torro-api-review` ohne Befund.

### M3.1 Mail lesen: Befehle und MCP
- [x] erledigt (freigegeben: `Session::uid_search_text` in `imap.rs`, Test-Fake in `mail/sync.rs`, `MailStore::thread_newest` in `store.rs`)
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
- [x] erledigt
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
- [x] erledigt (dazu `mail/mod.rs` für `pub mod send;`; offen: Detailansicht der Freigabe mit allen Empfängern
  braucht ein Feld in `ApprovalInfo` (torro-ipc), bis dahin nennt die Zusammenfassung jede Bcc-Adresse;
  Katalogtext von `mail.send_draft`: Bcc mitgezählt, Entwurf bleibt bei `unknown`)
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

### M3.5 Nacharbeit Senden und Kontoseite (aus den Berichten von M3.3/M5.2)
- [x] erledigt (dazu: neu `ui/server_fields.rs`; `ui/mod.rs` Modul, Haken im `Loaded`-Handler und Demo-Kennung im
  Test; Demo in `ui/launch.rs`; `details: Vec::new()` in `ApprovalInfo`-Literalen der Tests von `notify.rs`,
  `hook.rs`, `cli.rs`, `inbox.rs`, `torro-ipc/src/inbox.rs`)
- **Modell:** Opus · **Braucht:** M3.3, M5.2 · **Dateien:** `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torroworkplace/src/{core,approvals}.rs`, `crates/torroworkplace/src/mail/{send,service}.rs`,
  `crates/torroworkplace/src/ui/{approvals,accounts,wizard}.rs`, `docs/api.md`
- **Freigabe mit Einzelheiten:** `ApprovalInfo` bekommt das optionale Feld `details: Vec<String>` (verträglich);
  beim Senden alle Empfänger (An, Cc, Bcc), Anhänge mit Namen und Größe, der Textanfang – alles aus dem Entwurf im
  Kern. Die Seite „Approvals“ der Steuerzentrale zeigt sie aufklappbar. **API:** `approvals.list_approvals` (gibt es).
  **MCP:** dasselbe Tool zeigt die Einzelheiten nur dem Client, der gefragt hat (wie heute die Zusammenfassung).
- **Katalogtext `mail.send_draft`:** Bcc wird mitgezählt; bei `unknown` bleibt der Entwurf und wird nicht erneut
  gesendet.
- **Kalender später hinzufügen:** `connect_mail` behält die gespeicherte CalDAV-Adresse beim erneuten Verbinden,
  auch wenn Kalender und Aufgaben aus sind; mit `calendars`/`tasks` und `caldav` an einem bestehenden Konto werden
  sie geprüft und eingeschaltet (heute ignoriert). Die Kontoseite bietet dafür in „Server settings…“ die
  CalDAV-Adresse immer an.
- **Aufräumen:** Journal-Einträge „Kopie abgelegt“ ohne Entwurf werden beim Start entfernt; `ServerFields` und
  die Port-Logik aus `ui/wizard.rs` und `ui/accounts.rs` an einer Stelle; die Demo zeigt kein CalDAV-Konto neben
  dem Mail-Konto derselben Anmeldung; die Kontoseite übernimmt Änderungen von außen, solange kein Entwurf offen ist.
- **Abnahme:** Tests für Einzelheiten (nur aus dem Kern, nicht für fremde Clients), CalDAV-Adresse bleibt und
  Kalender lassen sich später einschalten, Journal-Aufräumen; Sichtprüfung Approvals und Kontoseite;
  `torro-api-review` ohne Befund.

### E4.1 Mail als Quelle im Eingang
- [x] erledigt
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
- [x] erledigt (zusätzlich: `torroinbox/src/main.rs` (`mod mail`, Grenze `MailSource`), `torroinbox/src/list.rs` (Helfer `pub(crate)`, Mail-Symbol), `Cargo.toml` von `torro-inbox` und `torroinbox` (`torro-mail`), Lucide-Symbole in `torro-gui/assets/icons/`)
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
- [x] erledigt (zusätzlich: `Cargo.toml` und `torroinbox/Cargo.toml` (`rfd` für die Dateiauswahl, `serde_json` und `tempfile` für Tests), `torro-inbox/src/demo.rs` (Demo-Antworten für `save_draft`/`send_draft`), Schalter „New“ in `torroinbox/src/app.rs`)
- **Modell:** Sonnet · **Braucht:** M3.3, E4.2 · **Dateien:** neu `crates/torroinbox/src/compose.rs`,
  `crates/torroinbox/src/{main,app}.rs`
- **Oberfläche:** Eigenes Fenster (`torroinbox --compose [--reply <id>|--forward <id>]`, Fensterklasse
  `torro-compose`, kachelt sich neben die Liste). Felder Von (Konto), An, Cc/Bcc (aufklappbar), Betreff, Text mit
  Signatur, Anhänge (Dateiauswahl). Antwort zitiert. Speichert automatisch als Entwurf (`mail.save_draft`, alle
  paar Sekunden nach Änderung), „Send“ (`mail.send_draft`), Fehler bleiben im Fenster, nichts geht verloren.
- **API/MCP:** nutzt `mail.save_draft`, `mail.send_draft`.
- **Abnahme:** Sichtprüfung (Breit, Schmal, hell/dunkel), Tests der Nachrichten; Senden-Fehler lässt den Entwurf stehen.

### E4.4 Mini, Leiste, Benachrichtigung für Mail
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** E4.2 · **Dateien:** `crates/torroinbox/src/glance.rs`,
  `crates/torroworkplace/src/inbox_notify.rs`
- **Oberfläche:** Mini/Leiste zeigen ungelesene Mails je Konto und die drei neuesten Absender; Desktop-Benachrichtigung
  für neue Mail in „Für mich“ mit „Archive“ und „Mark read“; Ruhezeiten (E3.4) gelten.
- **API/MCP:** nutzt vorhandene Befehle.
- **Abnahme:** Sichtprüfung Mini/Leiste, Tests der Nachrichten.

### E4.5 Schreibfenster über die ganze Breite (Issue #124)
- [x] erledigt (2026-10-14: `compose.rs` nimmt dem Formular die Lesebreite `size::content_max()`; Felder, Text und
  Signatur füllen die Breite der Kachel, Beschriftungen bleiben links in ihrer Spalte. Sichtprüfung auf macOS nur in
  der Startgröße möglich – Fenstergröße lässt sich ohne Bedienungshilfen-Recht nicht setzen)
- **Modell:** Sonnet · **Braucht:** E4.3 · **Dateien:** `crates/torroinbox/src/compose.rs`
- **Oberfläche:** Schreibfenster (Neu, Antworten, Weiterleiten). **API/MCP:** keine – Darstellung.
- **Abnahme:** Sichtprüfung Schmal/Mittel/Breit.

### E4.6 Formatierter Text beim Schreiben und Antworten (Issue #125)
- [x] erledigt (2026-10-14: Modell `Draft.html`, `Outgoing.html`, `ReadMessage.html`, `MailSettings.format`
  (`html` | `plain`, Standard `html`); neu `torro_mail::html` (säubern mit eigener Allowlist – Absätze, Umbrüche,
  Überschriften, fett/kursiv/unterstrichen/durchgestrichen, Code, Listen, Zitate, einfache Tabellen, Links nur `http`,
  `https`, `mailto`; keine Bilder, Stile, Skripte, Formulare, Attribute –, Klartext aus HTML, Signatur vor
  `<div class="torro-quote">`) und `torro_mail::markup` (Markdown-ähnliche Auszeichnung → HTML, Hervorhebung im
  Editor). `mime::build` schreibt `multipart/alternative` (text/plain aus dem HTML, dann text/html), auch mit Anhängen
  und Kalenderteil. Der Kern säubert beim Speichern und beim Senden (auch Entwürfe anderer Programme), der Klartext
  entsteht immer aus dem HTML; die Freigabe zeigt diesen Klartext. Schreibfenster: Werkzeugleiste (Fett, Kursiv,
  Unterstrichen, Link | Aufzählung, Nummeriert, Zitat), ⌘B/⌘I/⌘U/⌘K bzw. Strg, Enter setzt Listen fort, der Editor
  zeigt die Zeichen leise (`TEXT_3`) und Zitate in `TEXT_2`; Fett und Kursiv fragt er als Schnitt an, Iced 0.14 zeichnet
  im Editor auf macOS aber nur einen Schnitt; Demo-Zustand `--compose --demo --open format`. Antworten: Verlauf als `<blockquote type="cite">` mit „On … wrote:“, nicht als
  Auszeichnung gelesen; die Signatur steht im HTML und im Klartext über dem Verlauf. Steuerzentrale › Konto › Writing:
  „Write new mail as“ Formatted (HTML) / Plain text. Entscheidungen: eigene Allowlist statt neuer Abhängigkeit
  (`ammonia`); einfache Klartext-Konten behalten die Signatur am Ende (kein Verlauf-Marker im Klartext);
  Unterstreichen ist im Editor nur an den Zeichen `__` zu sehen. Offen (eigene Issues): Anhänge per Drag & Drop,
  Entwürfe wieder öffnen, verschachtelte Listen)
- **Modell:** Opus · **Braucht:** E4.3, E4.5 · **Dateien:** neu `crates/torro-mail/src/{html,markup}.rs`,
  `crates/torro-mail/src/{lib,account,message}.rs`, `crates/torro-mail-imap/src/mime.rs`,
  `crates/torroworkplace/src/mail/{write,send,sync}.rs`, `crates/torroworkplace/src/{core,approvals}.rs`,
  `crates/torroworkplace/src/ui/accounts.rs`, `crates/torro-ipc/src/{lib,api}.rs`, `crates/torroinbox/src/compose.rs`,
  `crates/torro-gui/src/icon.rs` (+ 5 Lucide-Icons), `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** Schreibfenster (Werkzeugleiste, Tasten), Steuerzentrale › Konten › Mail-Konto › Writing.
- **API:** `Request::SaveDraft` · `mail.save_draft` · Write – neues optionales Feld `draft.html` (gesäubert);
  `Request::UpdateMailAccount` · `accounts.update_mail_account` · Admin – neues optionales Feld `format`.
- **MCP:** `mail_save_draft` (mit `html`), `accounts_update_mail_account` (mit `format`).
- **Abnahme:** Tests: MIME mit beiden Teilen (mit und ohne Anhang), Klartext aus HTML, Säubern (Skripte, Bilder,
  Handler, `javascript:`), Zitat beim Antworten mit Signatur darüber, Kern speichert gesäubert und sendet beide Teile,
  Werkzeugleiste und Tasten auf dem Text; Sichtprüfung der Werkzeugleiste (dunkel/hell in der Startgröße; die Steuerzentrale-Zeile „Write new mail as“
  nicht angesehen, `--open account-detail` öffnet ein Nextcloud-Konto).

### M5.1 Steuerzentrale: Mail-Konto im Assistenten
- [x] erledigt
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
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** M2.7, M2.8 · **Dateien:** `crates/torroworkplace/src/ui/accounts.rs`
- **Oberfläche** (Einstellungen nach dem Vorbild TorroMail, ohne dessen Rechte und Cache-Stufen): Status je Dienst
  (Mail, Calendars, Tasks) mit letztem Abgleich; Absendername; Signatur (mehrzeilig); „Show new mail in Inbox“:
  All / Only „For me“ / Off; Dienste an/aus; „Change password…“ und „Server settings…“ (beide über
  `accounts.connect_mail`); nur bei offener Rolle einmal die Frage „Which folder holds sent mail?“ (`folder_roles` in
  `accounts.update_mail_account`); „Sync now“;
  „Remove account…“. Gespeichert über `accounts.update_mail_account`.
- **API/MCP:** nutzt M2.7, `accounts.sync`, `accounts.remove_account`.
- **Abnahme:** Sichtprüfung (Breit, Schmal, hell/dunkel), Tests der Nachrichten.

### M8.1 Mailsuche im Kern: Index, Tokenizer, Abfragesprache (Issue #136, Schritte 2–4)
- [x] erledigt
- **Modell:** Opus · **Braucht:** M3.1 · **Dateien:** `crates/torro-ipc/src/{lib,mail,api,inbox}.rs`,
  `crates/torroworkplace/src/mail/{mod,store,read,sync,service}.rs`, neu `mail/{fold,query,store_corpus}.rs`,
  `crates/torroworkplace/src/{core,pim,audit,daemon,mcp}.rs` (Request-Fall durchreichen),
  `tests/fixtures/mail/search-corpus.json`, `torro-core/src/i18n/de.rs`
- **Index:** Texte aller Mails bleiben im Suchindex; nur der Lesetext älterer Mails (über 90 Tage) wird weiter
  gelöscht. Ein Indexer je Konto holt ältere Texte (und solche über 5 MiB) nach der ersten Synchronisierung im
  Hintergrund nach, neueste zuerst, eine Nachricht je Griff auf die Befehlsverbindung (`sync::backfill`).
  Kopfzeilen aller Mails sind sofort durchsuchbar. Gesucht wird nur noch lokal (kein `UID SEARCH` mehr), also auch
  offline; solange Texte fehlen, sagt die Antwort `complete: false`, `incomplete_reason: indexing`.
- **Tokenizer:** Index und Suche falten gleich (`mail/fold.rs`): Groß/klein, Akzente, `ß` = `ss`,
  `ä` = `ae` = `a` (ö, ü). Zweiter FTS5-Index `trigram` für Wortteile ab 4 Zeichen (`rechnung` findet
  `Stromrechnung`). `sort: relevance` = BM25 (Betreff zählt am meisten) plus Aktualität. Hervorhebung aus dem
  Originaltext: Betreff und Textstelle um den ersten Treffer, in Stücken mit `hit`.
- **Abfragesprache** (`mail/query.rs`): Wörter, `"Phrasen"`, `-wort`, `OR`, `from:` `to:` `subject:` `filename:`
  `has:attachment` `in:` `before:` `after:` `is:unread|read|flagged|unflagged`.
- **Oberfläche:** Suchfeld über der Liste ist Schritt 1 von #136 (eigene Aufgabe); Vorschläge, Fortschritt
  und Hinweis „unvollständig“ Schritt 5. **API:** `mail.search_messages` mit `sort`; Antwort mit `complete`,
  `incomplete_reason`, `highlights`, `total_estimate` (alles optional, verträglich); neu `Request::MailIndexStatus`
  · `mail.index_status` · Read. **MCP:** `mail_search_messages`, `mail_index_status`.
- **Abnahme:** Parser-Tests (Operatoren, Phrasen, OR, Verneinung, Fehler); Testkorpus
  `search-corpus.json` (deutsch/englisch, HTML, Komposita, Umlaute, alte Mails) findet genau die erwarteten Mails
  zwischen Füllmails; Handler-Tests: Nachindizieren, offline, Hervorhebung, Zählung, Relevanz; Laufzeit-Test
  (3 000 Mails) und von Hand `cargo test --release -p torroworkplace -- --ignored search_budget` (100 000 Mails,
  < 100 ms).

### M8.2 Suchfeld über der Mail-Liste (Issue #136, Schritte 1 und 5)
- [x] erledigt (2026-10-14: neues Modul `crates/torroinbox/src/search.rs` und Ansicht `View::Search`. Das Feld steht
  über der Liste in jeder Ansicht des Mail-Fensters und in den Mail-Ansichten der anderen Fenster (sobald es ein
  Mail-Konto gibt); Lupe, `/` und ⌘F/Strg+F fokussieren es (⌘F über `keys::find_key`, auch wenn der Strg-Schutz des
  Fensters die Taste nimmt), wo kein Feld steht (Ansichten der Zentrale), öffnet die Lupe eine leere Suche mit den
  letzten Suchen. Standard: alle Ordner und Konten; aus einem Ordner heraus bietet die Leiste „All mail | This
  account | This folder“, dazu „Relevance | Date“ (Datum folgt „Newest/Oldest first“). Suche beim Tippen 150 ms nach
  der letzten Taste (`search_due` + `SearchTick`), Enter sofort. Vorschläge als Chips unter dem Feld (letzte Suchen,
  Absender/Empfänger der geladenen Mails als `from:`, Ordner als `in:`, Operatoren mit Tooltip), `Tab` nimmt den
  ersten. Letzte Suchen in `ui.json` (`recentSearches`, höchstens 8, `ui_state::save_searches_task`), gemerkt bei
  Enter und beim Öffnen eines Treffers, „Clear“ vergisst sie. Treffer hervorgehoben (`list::Hits`: Gewicht eine Stufe
  höher und `TEXT`, nie nur Farbe); die Vorschau zeigt die Fundstelle statt des Mailanfangs. Zeile darunter: Zahl der
  Treffer und – mit Icon und Worten – „Some matches may be missing …“ mit Indexstand aus `mail.index_status`.
  Escape: erst das Feld, dann Vorschläge, dann zurück zur Ansicht vor der Suche. Die Ordnersuche über
  `FolderView::query` entfällt, die Suche im Ordner ist jetzt der Bereich „This folder“. Entscheidungen: Vorschläge als
  Chips statt schwebendem Menü (verdecken die Treffer nicht, keine eigene Tastenführung), Gruppierung nach
  Konversation bleibt offen. Sichtprüfung auf macOS: Fenstergröße lässt sich ohne Bedienungshilfen-Recht nicht
  setzen, geprüft in Mittel, hell/dunkel, kompakt/bequem)
- **Modell:** Opus · **Braucht:** M8.1 · **Dateien:** `crates/torroinbox/src/{search,app,mail,list,keys,main}.rs`,
  `crates/torro-ipc/src/inbox.rs` (`InboxClient::find_mail`, `mail_index_status`), `crates/torro-gui/src/{ui_state,icon}.rs`,
  `crates/torro-gui/assets/icons/history.svg`, `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** Suchfeld über der Mail-Liste (alle drei Fenster). **API:** nutzt `mail.search_messages`
  (`Request::SearchMail` mit `account`/`folder`/`sort`) und `mail.index_status` – keine neuen Befehle. **MCP:**
  `mail_search_messages`, `mail_index_status` (gab es schon). Letzte Suchen bleiben im Client (Issue #136), kein Weg
  nach außen.
- **Abnahme:** Tests: Suche erst nach der Pause, Enter und geöffneter Treffer merken die Suche; Bereich Konto/Ordner und
  Sortierung gehen in die Anfrage; unvollständiger Index wird gesagt; Vorschläge je Eingabe, `Tab`; ⌘F auch, wenn der
  Schutz die Taste nahm; Escape zurück; letzte Suchen in `ui.json` neben dem Rest; Client baut die Anfrage richtig.
  Sichtprüfung `--demo --view mail --open search`.

### Danach
- **M7.5 Einladungen (iMIP):** `calendar.respond_invitation` · Write + „Nach außen“ · MCP
  `calendar_respond_invitation`; im Paneel „Accept / Maybe / Decline“, Termin in den Standardkalender, Antwort per Mail.
- **M7.6 Kontakte (CardDAV)**, Empfänger-Vorschläge im Schreibfenster.
- Sieve (Filter, Abwesenheit), OAuth für Google/Microsoft (braucht registrierte Client-IDs), JMAP.

## HTML-Mail (M9)

Entschieden 2026-10-14 (siehe [mail.md](../mail.md), Entscheidung 8): Mail wird als HTML gezeigt, nicht mehr
nur als Text. Das ersetzt „HTML nur zu Text umgewandelt“ (M2.4) und „Externe Bilder werden nie geladen“ (E4.2).
Darstellung in einem eigenen Fenster mit System-Webview, externe Bilder werden immer geladen.

```
M9.1 ─┬─ M9.2
      └─ M9.3   (M9.3 braucht das Fenster aus M9.2 zum Öffnen, die Demo-Daten nicht)

M9.4            (Versuch, unabhängig; ersetzt M9.2 nur, wenn er trägt und es entschieden ist)

M9.5 ─┬────────── M9.7 ─┬─ M9.9   (entschieden 2026-10-15: HTML im Lesebereich mit Blitz, Entscheidung 9)
M9.6 ─┘                 │
M9.5 ──── M9.8 ─────────┘
```

### M9.1 HTML im Kern: aufheben, bereinigen, herausgeben
- [x] erledigt (zusätzlich: `crates/torroworkplace/src/mail/store_corpus.rs` (Testhilfe `Body::of`); erlaubt auch `colspan`/`rowspan`, `face`/`size`; `docs/api-reference.md` gibt es noch nicht, A.9)
- **Modell:** Opus · **Braucht:** M3.1 · **Dateien:** `Cargo.toml` (`ammonia` in `[workspace.dependencies]`),
  `crates/torro-mail/src/message.rs`, `crates/torro-mail-imap/{Cargo.toml,src/mime.rs,tests/mime_roundtrip.rs}`,
  `crates/torroworkplace/src/mail/{store,read,sync,write}.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torroworkplace/src/{core,mcp}.rs`, `crates/torro-ipc/src/{inbox,mail}.rs` und
  `crates/torroinbox/src/compose.rs` (bauen `GetMail` aus, Feld ergänzen), `docs/api-reference.md`,
  `crates/torro-inbox/src/demo.rs` (Feld ergänzen)
- `Message` bekommt `html: Option<String>` (nur mit `include_html`) und `AttachmentInfo` `content_id:
  Option<String>` (ohne `<>`), beides optional, also verträglich. `mime.rs` liest neben dem Text den ersten
  HTML-Teil und bereinigt ihn mit `ammonia` über eine **Erlaubt-Liste**: Textauszeichnung und Gliederung (`p`,
  `div`, `span`, `br`, `hr`, `h1`–`h6`, `b`, `strong`, `i`, `em`, `u`, `s`, `small`, `sub`, `sup`, `font`,
  `center`, `blockquote`, `pre`, `code`, Listen), Tabellen (`table` bis `td`/`th`, `colgroup`, `col`, `caption`),
  `a` (nur `href` mit `http`, `https`, `mailto`), `img` (`src` mit `http`, `https`, `cid`, `data:image/png|jpeg|gif|
  webp`), das `style`-Element und die Attribute `style`, `class`, `id`, `align`, `valign`, `width`, `height`,
  `bgcolor`, `color`, `border`, `cellpadding`, `cellspacing`, `alt`, `title`, `dir`, `lang`. Alles andere fällt
  weg, ausdrücklich `script`, `iframe`, `object`, `embed`, `form`, `input`, `button`, `base`, `meta`, `link`
  (auch `rel=stylesheet`), `svg` und alle `on*`-Attribute. Obergrenze 2 MiB bereinigtes HTML; darüber ist
  `html` `None` und die Mail wird als Text gezeigt. Der Text (`text`, `from_html`) bleibt wie bisher, Agenten und Suche arbeiten weiter damit.
- Speicher: Spalte `html` in `bodies` (Migration, alte Zeilen `NULL`). Fehlt das HTML einer schon gecachten Mail,
  holt `mail.get_message` mit `include_html` den Inhalt neu vom Server (`BODY.PEEK`, kein `\Seen`).
- **API:** `Request::GetMail` bekommt `include_html: bool` (`#[serde(default)]`) · `mail.get_message` · Read;
  Beschreibung im Katalog ergänzen („with `include_html` also the cleaned HTML part, for showing it to a person;
  agents rarely need it, the text is enough. It is `untrusted` and may name pictures on the web“). Kein neuer
  Befehl.
- **MCP:** `mail_get_message` reicht `include_html` durch (Voreinstellung `false`).
- **Oberfläche:** M9.2 (Mail-Fenster), M9.3 (TorroInbox).
- **Abnahme:** Tests: Mail mit Text- und HTML-Teil → `text` aus dem Textteil, `html` bereinigt; Skript,
  `onclick`, `<form>`, `<iframe>`, `javascript:`-Link sind weg, `style` und `cid:`-Bild bleiben; `content_id` der
  Inline-Teile gesetzt; ohne `include_html` kein `html` in der Antwort; alte Cache-Zeile ohne HTML wird nachgeholt,
  ohne `\Seen` zu setzen; HTML über 2 MiB → `html: None`; Rechte wie `mail.get_message` (fremdes Konto →
  `not-found`). Protokoll ohne Inhalte: kein HTML, kein Ausschnitt (auch nicht in Fehlern der Bereinigung), keine
  `content_id`, Bild- oder Link-URLs, kein Betreff, kein Absender. API-Referenz neu erzeugt.

### M9.2 Mail-Fenster `torromailview`
- [x] erledigt (zusätzlich: `Cargo.lock`, `crates/torro-core/src/i18n/de.rs` (sechs neue Texte), in der Wurzel-`Cargo.toml`
  auch `muda` (Menü unter macOS: dort bekommt WKWebView ⌘C, `Esc`, ⌘W und Zoom nur über Menüeinträge), `gtk` (Linux:
  Programmname = Wayland-App-ID/X11-Klasse `torro-mailview`) und `base64` (Schriften des Designs als `data:`-URL); die Seite
  kommt über ein eigenes Protokoll `torro-page://localhost/` (WebView2 nimmt HTML-Text nur bis 2 MB), die Mail steht in
  einem deklarativen Shadow-Root mit Layout-/Paint-Containment, damit ihr CSS den Kopf nicht verdecken oder fälschen kann;
  Linux und Windows nicht im Fenster geprüft, Windows per `clippy --target x86_64-pc-windows-msvc` ohne Fehler)
- **Modell:** Opus · **Braucht:** M9.1 · **Dateien:** `Cargo.toml` (`wry`, `tao` in `[workspace.dependencies]`,
  Mitglied `crates/torromailview`), neu `crates/torromailview/` (`Cargo.toml`, `src/main.rs`, `src/page.rs`,
  `build.rs` wie bei den anderen Programmen), `scripts/ci-local.sh` und `.github/workflows/ci.yml`
  (Linux-Pakete `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`), `docs/mail.md` (nur falls sich beim Bau etwas
  Grundsätzliches ändert, dann vorher fragen)
- Eigenes Programm, eigener Prozess: `torromailview <m:id>` (Fensterklasse und App-ID `torro-mailview`, kachelt
  sich neben die Liste). tao + wry statt Iced, weil Iced kein HTML zeichnet und sich unter Wayland keine Webview in
  ein winit-Fenster einbetten lässt. Spricht nur über `torro_ipc` mit dem Kern (App-Schlüssel wie `torroinbox`),
  liest und schreibt nichts selbst. Einzige Ausnahme wie beim Öffnen von Anhängen: Die Datei, die
  `mail.get_attachment` im verwalteten Ordner abgelegt hat, liest es, um sie der Webview zu geben.
- Seite: `mail.get_message` mit `include_body` und `include_html`. Oben ein schlichter Kopf aus eigenem HTML (Von,
  An, Cc, Datum, Betreff, Anhänge als Liste; Farben und Schrift aus dem aktiven Design, hell/dunkel), darunter die
  Mail. Mail ohne HTML → Text in `<pre>`-artigem Block mit Zeilenumbruch. Alles aus der Mail wird escaped bzw. kommt
  nur als bereinigtes HTML aus M9.1.
- **Sicherheit im Fenster** (zweite Schicht nach der Bereinigung): Meta-CSP `default-src 'none'; script-src
  'none'; base-uri 'none'; form-action 'none'; img-src https: http: data: torro-cid:; style-src 'unsafe-inline' https: http:; font-src https: http:
  data:`; keine Skripte von uns in der Seite; wo wry es erlaubt, JavaScript abschalten (Linux über die
  WebKitGTK-Einstellungen). Navigation abfangen: Links (`http`, `https`, `mailto`) öffnen über `webbrowser` im
  Standardprogramm, die Seite selbst wechselt nie. Keine Downloads, keine neuen Fenster, kein Kontextmenü
  „Inspect“. Externe Bilder lädt die Webview direkt (Entscheidung 8: immer laden).
- `cid:`-Bilder: Seite schreibt `cid:x` in `torro-cid://x` um; das eigene Protokoll (wry custom protocol) holt den
  Teil über `mail.get_attachment` (Teil zur `content_id` aus `attachments`) und liefert die Datei aus dem verwalteten
  Ordner aus, nur `image/png`, `image/jpeg`, `image/gif`, `image/webp` (kein SVG), sonst 404.
- Tasten: `Esc`/`Ctrl+W` schließen, `Ctrl+C` kopiert die Auswahl, `Ctrl +`/`-`/`0` Zoom. Fehler (Kern weg,
  `not-found`) als einfache Seite mit `tr()`-Text. `--demo` zeigt eine Demo-HTML-Mail ohne Kern.
- **API/MCP:** keine neuen Befehle (nutzt `mail.get_message`, `mail.get_attachment`).
- **Abnahme:** Tests: Kopf escaped Absender/Betreff mit `<script>`; CSP steht in jeder Seite; `cid:` wird
  umgeschrieben, Nicht-Bild-Teil → 404; Navigation zu fremder URL wird abgewiesen und an den Browser gegeben (Logik
  als reine Funktion getestet); SVG über `torro-cid://` → 404. Protokoll ohne Inhalte: keine Link- oder Bild-URLs,
  keine `content_id`, kein Betreff, kein Absender, kein HTML. Baut auf Linux, macOS, Windows (`scripts/ci-local.sh`). Sichtprüfung mit `--demo`
  (Newsletter mit Tabellen und Bildern, reine Textmail), hell/dunkel; im Bericht die neuen Übersetzungen nennen.

### M9.3 TorroInbox: HTML-Mail öffnen
- [x] erledigt (zusätzlich: `dist-workspace.toml` (`[dist.dependencies.apt]` mit `libwebkit2gtk-4.1-dev`,
  `libgtk-3-dev` für den Linux-Release von M9.2), `crates/torro-core/src/i18n/de.rs`; das Paneel fragt
  `mail.get_message` mit `include_html` und fällt ohne HTML auf die reine Textabfrage zurück (offline, alte
  Cache-Zeile); `Enter` und Doppelklick öffnen das Paneel wie bisher und das Fenster, sobald die Mail HTML hat;
  Demo: `m:p2` (aus HTML) und `m:w14` (Text und HTML), „View as HTML“ startet dort `torromailview --demo`)
- **Modell:** Sonnet · **Braucht:** M9.1, M9.2 · **Dateien:** `crates/torroinbox/src/{mail,keys,app}.rs`,
  `crates/torro-inbox/src/demo.rs` (Demo-Newsletter mit `html`), `crates/torro-core/src/i18n/de.rs` (nur wenn nicht
  parallel, sonst im Bericht)
- **Oberfläche:** Das Paneel zeigt weiter Kopf, Anhänge und Knöpfe. Hat die Mail HTML, steht über dem Text der Knopf
  „View as HTML“ (Taste `v`), und `Enter` bzw. Doppelklick in der Liste öffnet die Mail direkt im Mail-Fenster
  (`torromailview <id>`, wie `--compose` gestartet). Der Hinweis „Made from HTML. Pictures and other content from
  the web are not loaded.“ entfällt; der Text im Paneel ist nur noch Vorschau. Eintrag im Eingang gilt beim Öffnen
  des Fensters als geöffnet (wie heute über `mail.get_message` mit dem App-Schlüssel).
- `apps.open_app` mit Ziel `mail:` öffnet weiter `torroinbox --view mail` (Fußnote 13 in [api.md](../api.md)):
  Das ist die Mail-Ansicht des Eingangs, `torromailview` zeigt nur eine einzelne Mail und wird aus ihr heraus
  geöffnet. Agenten brauchen das HTML-Fenster nicht, sie lesen über `mail_get_message`.
- **API/MCP:** nutzt M9.1, keine neuen Befehle.
- **Abnahme:** Sichtprüfung mit `--demo` (Breit/Mittel/Schmal, hell/dunkel); Tests der Nachrichten: `v` und
  `Enter` starten das Fenster nur bei Mails mit HTML, ohne HTML bleibt alles wie bisher; Übersetzung „View as HTML“.
  Protokoll ohne Inhalte (keine Kennung im Klartext neben Betreff oder Absender).

### M9.4 Versuch: HTML-Mail mit Blitz direkt in Iced zeichnen
- [x] erledigt (Weg 2 läuft, Weg 1 geht mit wgpu 27 gegen 30 nicht; Empfehlung „erst später“, siehe
  [research.md](../research.md), Abschnitt 8. Abweichend: `exclude = ["spikes"]`, weil Cargo in `exclude` keine
  Muster kennt; zusätzlich `spikes/blitz-iced/.gitignore` (`/target`), `fixtures/cid/` (Bild für `cid:`) und
  `shots/` (Screenshots); die lange Mail entsteht im Code statt als 1-MiB-Datei; Linux nicht geprüft)
- **Modell:** Opus · **Braucht:** nichts (nutzt eigene Beispiel-Mails, nicht M9.1) · **Dateien:** neu
  `spikes/blitz-iced/` (eigenes Cargo-Projekt: `Cargo.toml`, `src/main.rs`, `fixtures/*.html`), `Cargo.toml` der
  Wurzel (nur `exclude = ["spikes/*"]`), `docs/research.md` (neuer Abschnitt mit dem Ergebnis)
- **Warum:** Iced zeichnet kein HTML, deshalb plant M9.2 ein eigenes Fenster mit System-Webview. Blitz (Dioxus:
  Stylo für CSS, Taffy für Layout, Vello zum Zeichnen) stellt HTML und CSS ohne JavaScript und ohne Webview dar.
  Trägt das, kann die Mail als Widget im Iced-Fenster stehen: kein zweites Fenster, keine Grenze unter Wayland,
  keine WebKitGTK-Pakete, kein JavaScript, das abzuschalten wäre.
- **Versuch, kein Produktcode:** Das Projekt liegt außerhalb des Workspace (`exclude`), damit seine Abhängigkeiten
  (Blitz-Crates, ggf. andere `wgpu`-Version) weder Workspace noch CI berühren. Workspace-Regeln gelten sinngemäß
  (kein `unsafe`, kein `unwrap()`), Tests sind nicht nötig. Am Ende wird nichts davon in `crates/` übernommen; das
  wäre eine eigene Aufgabe nach der Entscheidung.
- Ein kleines Iced-Programm (gleiche Iced-Version wie der Workspace) zeigt links eine Liste der Beispiel-Mails,
  rechts die gewählte Mail, gezeichnet mit Blitz. Zwei Wege probieren, den ersten zuerst:
  1. **Gemeinsame GPU:** Blitz/Vello zeichnet über Iceds `shader`-Widget in dieselbe `wgpu`-Instanz. Vorher
     prüfen, ob die `wgpu`-Versionen von Iced und Vello zusammenpassen; wenn nicht, notieren und Weg 2.
  2. **Bild:** Blitz zeichnet per CPU (Vello CPU bzw. `anyrender`-CPU-Backend) in ein Pixelbild, Iced zeigt es
     als `image`; neu zeichnen bei Größenänderung und Scrollen.
- Mindestens: Breite folgt dem Fenster (Neu-Layout beim Ziehen), Scrollen, Klick auf Link wird erkannt (URL nur
  ausgeben, nicht öffnen), externe Bilder werden geladen (Blitz-Netzwerk), `cid:`-Bilder aus einer Datei neben der
  Fixture, Text markieren und kopieren (wenn Blitz es kann, sonst notieren), hell/dunkel.
- **Beispiel-Mails** unter `fixtures/`, nur selbst gebaute oder öffentlich zugängliche Newsletter, **keine echten
  privaten Mails**: Newsletter mit verschachtelten Tabellen und Bildern, Rechnung eines Shops, typische
  Outlook-Mail (`<o:p>`, MSO-Kommentare, `font`), Mail mit `<style>`-Block und Media Queries, schlichte Textmail
  als `<pre>`, sehr lange Mail (über 1 MiB). Jeweils mit `ammonia` wie in M9.1 bereinigt, damit der Versuch das
  HTML sieht, das später wirklich ankommt.
- **API/MCP:** keine. Ein Versuch ohne Funktion für Nutzer; er nutzt keinen Request und ändert keinen. Kommt Blitz
  später ins Produkt, ändert sich nur die Oberfläche von M9.2/M9.3, die Befehle aus M9.1 bleiben.
- **Abnahme:** Abschnitt „Blitz in Iced (Versuch M9.4)“ in `docs/research.md` mit: welcher Weg lief, Versionen
  (Iced, Blitz, `wgpu`, Vello), Screenshots je Beispiel-Mail neben derselben Mail in einem Browser, Zeit für das
  erste Zeichnen und beim Neu-Layout (grob, auf dem eigenen Rechner), Größe des Release-Binaries mit und ohne
  Blitz, was fehlt oder falsch aussieht, Lizenzen der neuen Abhängigkeiten (Stylo ist MPL-2.0) und eine klare
  Empfehlung: **Blitz statt `torromailview`**, **erst später** oder **nicht**. Läuft der Versuch auf macOS und
  Linux (Hyprland)? Windows nur, wenn es ohne Umwege geht. Entschieden wird danach mit dem Menschen, nicht in
  dieser Aufgabe.

### M9.5 Mail-Darstellung als Einstellung, alte HTML-Attribute als CSS
- [x] erledigt (zusätzlich: `crates/torro-mail-imap/tests/mime_roundtrip.rs` (eine Erwartung von M9.1 nennt jetzt das
  CSS statt `bgcolor`/`cellpadding`), `crates/torroworkplace/src/ui/general.rs` (Test baut `Settings` vollständig);
  Typen `torro_core::settings::MailFormat` (`html`/`text`) und `MailAppearance` (`app`/`light`/`dark`). Die Umschreibung
  läuft auf dem schon bereinigten HTML, danach noch einmal durch die Erlaubt-Liste; über die Aufgabe hinaus: `align` bei
  `img` (`float`/`vertical-align`) und `hr`, `align=middle` wie `center`, Tabellen in zentrierten Zellen oder `<center>`
  werden zentriert wie im Browser; `border` außer an `table` und `color` außer an `font` bleiben Attribute;
  `docs/api.md` zählt die Einstellungen nicht auf, keine Änderung)
- **Modell:** Opus · **Braucht:** M9.1 · **Dateien:** `crates/torro-core/src/settings.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torroworkplace/src/settings_service.rs`,
  `crates/torro-mail-imap/src/mime.rs`, `docs/api-reference.md`, `docs/api.md` (Tabelle der Einstellungen, falls dort
  aufgezählt)
- Zwei neue Einstellungen in `Settings` (lesend nachsichtig wie `assistant_default`):
  - `mail_view`: `html` (Voreinstellung) oder `text` – wie eine Mail im Lesebereich zuerst erscheint.
  - `mail_appearance`: `app` (Voreinstellung: wie die Darstellung der Suite, also System/Hell/Dunkel aus
    `appearance`), `light` oder `dark` – nur für den **Inhalt** der Mail, nie für die übrige Oberfläche.
- `SettingKey::MailView` und `SettingKey::MailAppearance` (neue Werte, verträglich), Doc-Kommentar mit den Werten;
  offene Fenster folgen (wie bei `appearance`).
- **Bereinigung schreibt alte Attribute in CSS um** (`clean_html`, M9.1), weil Blitz sie nicht kennt (Versuch M9.4)
  und weil es die Darstellung überall vereinheitlicht: `cellpadding` → `padding` der Zellen, `cellspacing` →
  `border-spacing`, `border` an `table` → Rahmen von Tabelle und Zellen, `bgcolor` → `background-color`,
  `width`/`height` (Zahl → px, Prozent bleibt), `align`/`valign` → `text-align`/`vertical-align` (bei `table` mit
  `align=center` → `margin: auto`), `<center>` → `div` mit `text-align:center`, `<font face size color>` → `span`
  mit `font-family`, `font-size` (1–7 nach HTML-Tabelle) und `color`. Bestehendes `style` gewinnt (es wird hinten
  angehängt). Werte werden geprüft (Farben, Zahlen, Schriftnamen ohne `;{}()`), Unpassendes fällt weg.
- **API:** `settings.set_setting` mit `mail_view` / `mail_appearance` · Admin (wie die übrigen Einstellungen) ·
  **MCP:** `settings_set_setting` (vorhanden, nimmt die neuen Schlüssel ohne eigenen Code). **Oberfläche:** M9.7
  (Schalter im Lesebereich), M9.8 (Einstellungsseite).
- **Abnahme:** Tests: Voreinstellungen `html`/`app`; Setzen und Lesen beider Schlüssel, falsche Werte → `error`;
  alte Einstellungsdatei ohne die Felder liest sich; Umschreibung je Attribut (auch: vorhandenes `style` gewinnt,
  bösartige Werte wie `red;background:url(x)` fallen weg); Tests von M9.1 laufen weiter. API-Referenz neu erzeugt.

### M9.6 Bibliothek `torro-htmlview`: HTML mit Blitz in Iced zeichnen
- [x] erledigt (zusätzlich: `Cargo.lock`; in `[workspace.dependencies]` außer Blitz/anyrender auch `peniko`, `parley`
  (gemeinsamer Schrift-Kontext), `cursor-icon`, `image` (Blitz schaltet selbst keinen Decoder ein: PNG, JPEG, GIF,
  WebP) und `png` (nur das Beispiel); Lizenzliste gibt es nicht (kein `deny.toml`). Abweichend: eine Mail, die breiter
  ist als der Platz (fester 600-px-Newsletter), wird bis 0,5 verkleinert statt abgeschnitten; Linien folgen beim
  Umfärben der Fläche unter ihnen. Beispiel: `cargo run -p torro-htmlview --example fixtures [-- --shoot <ordner>]`.
  Linux nicht gebaut, Windows per `cargo xwin clippy` ohne Fehler)
- **Modell:** Opus · **Braucht:** nichts (eigene Beispiele; nutzt das Wissen aus M9.4) · **Dateien:** `Cargo.toml`
  (Blitz-Crates, `anyrender`, `anyrender_vello_cpu` in `[workspace.dependencies]`, Mitglied), neu
  `crates/torro-htmlview/` (`Cargo.toml`, `src/lib.rs` und Module, `tests/fixtures/`), `deny.toml` bzw.
  Lizenzliste, falls vorhanden (MPL-2.0 für Stylo)
- Ein Iced-Widget `HtmlView`, das bereinigtes HTML (aus M9.1/M9.5) mit Blitz per CPU zeichnet (Weg 2 aus M9.4:
  Blitz → Pixelbild → Iced `image`), mit gemeinsamem Schrift-Kontext für alle Mails. Breite folgt dem Platz
  (Neu-Layout beim Ziehen), Höhe = Inhaltshöhe, damit der Lesebereich wie bisher scrollt; Mauszeiger über Links,
  Text markieren und kopieren.
- **Kein `blitz-net`** (bringt OpenSSL): Bilder holt die Bibliothek selbst über das `reqwest` des Workspace (rustls),
  nur `http`/`https`, nur Bildtypen (an den ersten Bytes erkannt, kein SVG), höchstens 10 MiB je Bild und 50 MiB
  je Mail, Zeitlimit, keine Cookies, kein `Referer`. Externe Bilder werden immer geladen (Entscheidung 8). Bilder
  hinter einer Weiterleitung müssen erscheinen (Fehler aus M9.4). `cid:`-Bilder liefert die App über einen
  Rückruf (Trait), die Bibliothek fragt nie selbst den Kern.
- **Hell/Dunkel nur für den Inhalt:** `light` zeichnet das Original. `dark` färbt beim Zeichnen um (Hülle um die
  Zeichenfläche von `anyrender`): helle Hintergründe werden dunkel, dunkle Schrift hell, Farbton bleibt, Kontrast
  mindestens 4,5:1 für Text; **Bilder bleiben unverändert**. Hintergrund der Fläche aus dem Design.
- Ausgaben als Nachrichten: Klick auf Link (`http`, `https`, `mailto`) → `LinkClicked(url)`, die App öffnet ihn;
  sonst nichts. Kein JavaScript (Blitz hat keins). Protokoll ohne Inhalte, URLs und Mailtext.
- **API/MCP:** keine (reine Darstellung, kein Request). Oberfläche: M9.7.
- **Abnahme:** Tests ohne Fenster: Fixtures (Newsletter mit Tabellen, Rechnung, Outlook-Mail nach M9.5-Umschreibung,
  `<style>` mit Media Query, lange Mail) zeichnen ohne Fehler in ein Bild fester Breite; Breite 360/700 px ändert
  das Layout; Dunkel-Umfärbung: Weiß → dunkel, Schwarz → hell, Bildpixel unverändert, Kontrast eingehalten;
  Bildabruf: SVG/HTML abgewiesen, Größengrenze, Weiterleitung folgt (lokaler Testserver wie `httpmock`, kein
  Netz); Link-Treffer an Koordinaten. `cargo build` auf macOS; Linux/Windows ohne Systempakete (kein GTK).

### M9.7 TorroInbox: HTML im Lesebereich, Schalter HTML/Text und Hell/Dunkel
- [x] erledigt (zusätzlich: `Cargo.lock`; `tokio` in `crates/torroinbox/Cargo.toml`, weil der `cid:`-Abruf auf dem
  Arbeits-Thread der Bibliothek auf den Kern wartet. Startparameter `--mail-view html|text` und
  `--mail-colors app|light|dark` (wie `--appearance`, nichts wird gespeichert) für Sichtprüfung und Vorlagen. Die
  Demo-Mails: Newsletter `m:p2` mit vier `cid:`-Bildern (die selbstgemachten PNGs aus den Fixtures von
  `torro-htmlview`, per `include_bytes!`), Outlook-Antwort `m:w2` (Text- und HTML-Teil), Textmail `m:w1`. Unter
  dem HTML steht statt „shown as plain text“ ein eigener Hinweis; „Links“ unter dem Text nur in der Text-Ansicht.
  `mailto:` geht wie `http(s)` an `webbrowser`, der es auf macOS dem Standard-Browser gibt)
- **Modell:** Opus · **Braucht:** M9.5, M9.6 · **Dateien:** `crates/torroinbox/{Cargo.toml,src/mail.rs,src/app.rs,
  src/keys.rs}`, `crates/torro-inbox/src/demo.rs`, `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** Der Lesebereich (Leser neben der Liste und Paneel) zeigt eine Mail mit HTML gleich als HTML
  (`HtmlView`), wenn `mail_view = html`; sonst den Text wie bisher. Über dem Inhalt zwei kleine Schalter:
  „HTML | Text“ (nur bei Mails mit HTML; Taste `v` schaltet um) und „Light | Dark“ für den Mail-Inhalt (nur in
  der HTML-Ansicht; Voreinstellung aus `mail_appearance`, bei `app` die aktuelle Darstellung der Suite). Die
  Schalter gelten für diese Ansicht, bis die Einstellung geändert wird; dauerhaft stellt man es in M9.8 ein.
- Links öffnen im Standard-Browser (`webbrowser`), `cid:`-Bilder über `mail.get_attachment` (nur Teile mit
  `content_id` dieser Mail). Markieren und „Copy text“ funktionieren in beiden Ansichten.
- Knopf „View as HTML“ und das Starten von `torromailview` (M9.3) fallen weg; `Enter`/Doppelklick tun wieder nur,
  was sie vor M9.3 taten.
- **API/MCP:** nutzt `mail.get_message` (`include_html`), `mail.get_attachment`, `settings.get_settings`; keine
  neuen Befehle.
- **Abnahme:** Tests der Nachrichten: Voreinstellung `html` zeigt HTML, `text` zeigt Text; `v` schaltet nur bei
  Mails mit HTML; Hell/Dunkel-Schalter ändert nur den Mail-Inhalt; `app` folgt der Darstellung; Link-Klick ruft
  den Browser (Test über eine austauschbare Funktion); kein Start von `torromailview` mehr. Sichtprüfung mit
  `--demo` (Newsletter, Outlook-Mail, Textmail) Breit/Mittel/Schmal, App hell und dunkel, Mail hell und dunkel.

### M9.8 Einstellungsseite: Darstellung von Mails
- [x] erledigt (Gruppe „Mail“ am Ende der Seite Appearance, beide Einstellungen als Auswahlliste wie „Write new mail as“
  im Mail-Konto; „Mail colors“ in amerikanischer Schreibung wie „Color mode“; Sichtprüfung auf macOS per Screenshot
  des eigenen Fensters (Breit, Mittel, Schmal, hell/dunkel, Englisch/Deutsch))
- **Modell:** Sonnet · **Braucht:** M9.5 · **Dateien:** `crates/torroworkplace/src/ui/general.rs` (Seite
  Appearance), `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** Auf der Seite **Appearance** der Steuerzentrale eine Gruppe „Mail“: „Show mail as“ – Formatted
  (HTML) / Plain text, und „Mail colours“ – Like the app / Light / Dark, mit dem Hinweis, dass dunkel
  umgefärbte Newsletter manchmal schlechter aussehen. Gespeichert über `settings.set_setting`. Zieht mit S.20
  später in die Einstellungen der Inbox um.
- **API/MCP:** nutzt M9.5.
- **Abnahme:** Tests: Auswahl schickt `SetSetting` mit `mail_view` bzw. `mail_appearance`; Sichtprüfung der Seite
  (Breit, Schmal, hell/dunkel).

### M9.9 Mail-Fenster `torromailview` zurückbauen
- [x] erledigt (`base64` bleibt, torro-htmlview nutzt es; `muda` bleibt im Lock über tray-icon; `docs/mail.md`: Entscheidungen 8/9, Funktionstabelle war schon richtig)
- **Modell:** Sonnet · **Braucht:** M9.7 · **Dateien:** `Cargo.toml` (Mitglied und `wry`, `tao`, `muda`, `gtk`,
  ggf. `base64` entfernen, wenn sonst ungenutzt), `crates/torromailview/` (löschen), `Cargo.lock`,
  `dist-workspace.toml` (`[dist.dependencies.apt]` entfernen), `.github/workflows/ci.yml` und `scripts/ci-local.sh`
  (Linux-Pakete entfernen), `crates/torro-core/src/i18n/de.rs` (Einträge nur des Fensters entfernen),
  `docs/mail.md` (Funktionstabelle)
- Nach M9.7 zeigt niemand mehr das Fenster an. Mit ihm fällt die einzige Systemabhängigkeit (WebKitGTK) weg.
- **API/MCP:** keine Änderung (`include_html` und `mail.get_attachment` bleiben, TorroInbox nutzt sie).
- **Abnahme:** Workspace baut ohne `wry`/`tao`; `cargo tree` enthält kein `webkit2gtk`/`gtk`; `scripts/ci-local.sh`
  grün; keine Verweise mehr auf `torromailview` außerhalb von `docs/tasks/` und `docs/research.md`.

### Später zu M9
- Antworten und Weiterleiten mit dem HTML der Vorlage (zitiert als HTML), Schreiben mit einfacher Formatierung.
- Schalter „Load pictures from the web“ (immer / pro Absender / nie), falls das Laden stört.

## M10: Mail-Bestand, Kategorisierungsaufträge und lokaler Speicher (Issue #263)

Die Gestaltung steht in `docs/prototypes/mail-263/`. Die Werte dort sind Beispieldaten. Der Kern liefert echte
Zahlen und führt alle Änderungen aus. Die Aufgaben werden in der angegebenen Reihenfolge bearbeitet; jede neue
Funktion hat Oberfläche, Request mit Katalogeintrag und MCP-Weg. Die bereits vorhandenen Befehle bleiben lesbar.

### M10.1 Konto- und Ordnerstatistik im Kern
- [x] erledigt (2026-10-21): lokale Konto- und Ordnerzahlen, Datumsgrenzen, getrennte Indexzustände und zuordenbarer Anhangscache über `mail.account_stats`; nicht belegte Servervollständigkeit bleibt ausdrücklich offen.
- **Modell:** Opus · **Braucht:** M2.6, M8.1 · **Dateien:** `crates/torro-ipc/src/{lib.rs,mail.rs,api.rs}`,
  `crates/torroworkplace/src/mail/{store.rs,service.rs,read.rs}`, `crates/torroworkplace/src/mcp.rs`,
  `crates/torro-settings-ui/src/demo.rs`, `docs/api-reference.md`
- **Oberfläche:** Kontoseite (M10.2). **API:** neuer lesender Request `MailAccountStats` ·
  `mail.account_stats` · Read. **MCP:** `mail_account_stats`.
- Der Store zählt Mails, gesehen/ungesehen, Ordner, ältestes/neuestes Datum, lokal vorhandene Texte und die drei
  disjunkten Indexzustände „vollständig“, „teilweise“, „ohne Text“. Je Ordner dieselben Zähler. Serverseitig
  gemeldete Ordnerzahlen nur dann als Serverzahlen ausgeben, wenn sie tatsächlich vom Server stammen; sonst die
  lokalen Zahlen ausdrücklich als solche kennzeichnen. Unvollständige Synchronisierung markiert älteste/neuste
  Zeit als „bekannt“ statt als vollständig. Größen von SQLite und Anhangscache separat ausweisen.
- Ein Leserequest gibt nur Konten in Reichweite zurück; keine fremden Zahlen über `inbox`-Rechte. Bestehendes
  `mail.index_status` bleibt verträglich, zeigt aber „ohne Text“ nicht länger als vollständig indiziert an.
- **Abnahme:** 24.813 = 23.565 + 1.248 und Indexzustände summieren sich immer zur lokalen Mailzahl; leere und
  teilweise synchronisierte Ordner werden korrekt bezeichnet; API/MCP begrenzen Kontenrechte.

### M10.2 Kontoseite mit Bestand und Ordnern
- [x] erledigt
- **Modell:** Opus · **Braucht:** M10.1 · **Dateien:** `crates/torro-settings-ui/src/{mail.rs,demo.rs}`,
  `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** Mail-Einstellungen › Accounts › Konto nach `docs/prototypes/mail-263/screen-account.png`, mit
  Gesamtzahl, gelesen/ungelesen, Ordnerzahl, ältester/neuster bekannter Mail, Sync-Zustand, lokaler Abdeckung
  und aufklappbaren Ordnerzahlen. Unterschied zwischen Server-, lokalen und ungelesenen Zahlen im Text erklären.
- **API/MCP:** `mail.account_stats` aus M10.1; keine eigene Zählung in der Oberfläche.
- **Abnahme:** Breit/Mittel/Schmal sowie leer, lädt, unvollständig und Fehler; sichtbare Zahlen entsprechen der
  API-Antwort und die Kontoauswahl verliert keine bestehenden Absender-/Signaturfelder.

### M10.3 Steuerbarer, fortsetzbarer Kategorisierungsauftrag
- [x] erledigt
- **Modell:** Opus · **Braucht:** M10.1, E12.6 · **Dateien:** `crates/torro-ipc/src/{lib.rs,inbox.rs,api.rs}`,
  `crates/torroworkplace/src/{core.rs,mcp.rs,categorize.rs}`, `crates/torroworkplace/src/mail/{store.rs,service.rs}`,
  `crates/torroworkplace/src/approvals.rs`, `crates/torroworkplace/src/audit.rs`, `docs/api-reference.md`,
  `docs/kategorien.md`
- **Oberfläche:** Umfang, Vorschau und Laufstand (M10.4). **API:** gezielte `inbox.*`-Requests zum Vorberechnen,
  Starten und Steuern (Pause/Fortsetzen/Abbruch) eines Auftrags sowie zum Lesen seines Zustands; Zugriff Read
  für Vorschau/Status, Write für Start/Steuerung. **MCP:** gleichnamige `inbox_*`-Tools.
- Konten, Ordner und Zeitraum einschließlich „alle“ wählen; die bisherige 30-Tage-/500er-Grenze bleibt nur
  voreingestellter Umfang des alten Befehls. Vorschau trennt offen, automatisch kategorisiert und manuell/per
  Regel geschützt. Erneutes Prüfen betrifft nur automatische Kategorien. Ein Job mit Kennung, festgehaltenem
  Umfang, Fortschritt und Fehlern bleibt nach Neustart fortsetzbar. In Batches arbeiten; Kategorie nur setzen,
  wenn ihre Quelle seit dem Lesen nicht manuell oder durch Regel geändert wurde. Modell und Cloud-Ort zeigen.
- **Abnahme:** Alle Mails sind ohne festes Limit erreichbar; Abbruch und Neustart verlieren weder manuelle
  Zuordnungen noch den Jobstand; normale neue Mail und zwei Clients können keine Kategorie überschreiben.

### M10.4 Kategorisierungsseite mit Umfang und Fortschritt
- [x] erledigt
- **Modell:** Opus · **Braucht:** M10.3 · **Dateien:** `crates/torro-settings-ui/src/{mail.rs,demo.rs}`,
  `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** `docs/prototypes/mail-263/screen-sort.png`: Konto/Ordner/Zeitraum/Modell wählen,
  Zahlenvorschau, Modellstandort und tatsächlichen Jobstand samt Fehlern, Pause, Fortsetzen, Abbruch zeigen.
- **API/MCP:** M10.3-Befehle; kein lokaler Jobzustand außer Formularwerten.
- **Abnahme:** Seitenleisten-Badges als ungelesen und Vorschau/Fortschritt als alle Mails erklärt; ein
  Neustart rekonstruiert den Job aus dem Dienst; leere und Fehlerzustände sind verständlich.

### M10.5 Alle Kategorien in einem Auftrag entfernen
- [x] erledigt
- **Modell:** Opus · **Braucht:** M10.3 · **Dateien:** `crates/torro-ipc/src/{lib.rs,inbox.rs,api.rs}`,
  `crates/torroworkplace/src/{core.rs,mcp.rs,approvals.rs,audit.rs}`, `crates/torroworkplace/src/mail/{store.rs,service.rs}`,
  `crates/torro-settings-ui/src/{mail.rs,demo.rs}`, `crates/torro-core/src/i18n/de.rs`, `docs/api-reference.md`
- **Oberfläche:** Bestätigung nach `docs/prototypes/mail-263/screen-reset.png` mit Anzahl je Konto; danach
  Fortschritt. **API:** `inbox.clear_categories` · Delete mit ausdrücklichem Umfang „alle Konten“ und Option,
  die automatische Kategorisierung auszuschalten; `inbox.get_categorization` liest den Jobstand.
  **MCP:** `inbox_clear_categories` (zentrale Freigabe für Delete).
- Automatische und manuelle Kategorien sowie zugehörige Modellantworten entfernen, ohne Mails, Ordner oder
  Sortierregeln zu löschen. Vorher laufende Sortierjobs beenden; während des Löschens darf keiner neu starten.
  Der Auftrag ist in Batches wiederaufnehmbar. Ohne deaktivierte Automatik können neue/erneut verarbeitete Mails
  wieder Kategorien erhalten; der Dialog sagt dies ausdrücklich.
- **Abnahme:** Der Auftrag entfernt sämtliche Kategoriequellen aller zugänglichen Konten, zählt die Änderungen,
  ist nach Abbruch/Neustart nachvollziehbar und lässt Maildaten und Regeln unverändert.

### M10.6 Cache-Strategie und Indexwartung im Kern
- [x] erledigt
- **Modell:** Opus · **Braucht:** M10.1 · **Dateien:** `crates/torro-ipc/src/{lib.rs,mail.rs,api.rs}`,
  `crates/torroworkplace/src/{core.rs,mcp.rs}`, `crates/torroworkplace/src/mail/{mod.rs,store.rs,service.rs,sync.rs,read.rs}`,
  `crates/torro-core/src/settings.rs`, `crates/torroworkplace/src/approvals.rs`, `crates/torroworkplace/src/audit.rs`,
  `docs/api-reference.md`, `docs/mail.md`
- **Oberfläche:** Cache- und Indexseite (M10.7). **API:** `mail.get_cache_policy` Read,
  `mail.set_cache_policy` Admin, `mail.start_cache_job` Write und `mail.cache_job_status` Read.
  **MCP:** entsprechende `mail_*`-Tools.
- Je Konto „nur Kopfzeilen“, „letzte 90 Tage“ oder „alles offline“; Indextexte bleiben unabhängig von
  Lesetext-Retention. Optionales Anhangs-Nachladen mit separater Aufbewahrung. Speicherbedarf vor Anwendung
  abschätzen. Fehlende Texte laden, Index neu aufbauen oder entbehrlichen Cache bereinigen; keine Entwürfe,
  Sendejournale oder Servermails löschen. Jobs mit Kennung, Fortschritt und Wiederaufnahme.
- **Abnahme:** Wechsel der Strategie und Neustart behalten die Wahl; Offline-/Indexzähler entsprechen dem
  tatsächlichen Bestand; Bereinigung verliert keine Entwürfe und zeigt vorher ihre Wirkung.

### M10.7 Cache- und Indexseite
- [x] erledigt
- **Modell:** Opus · **Braucht:** M10.6 · **Dateien:** `crates/torro-settings-ui/src/{mail.rs,demo.rs}`,
  `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** `docs/prototypes/mail-263/screen-storage.png`: Strategie, getrennte Speichergrößen,
  vollständig/teilweise/ohne Text, ältester lokaler Mailtext und Wartungsaktionen samt Vorschau/Fortschritt.
- **API/MCP:** M10.1 und M10.6; alle Änderungen gehen über den Dienst.
- **Abnahme:** Der Nutzer erkennt, was vollständig offline lesbar und durchsuchbar ist; ein Fehler oder eine
  langsame Wartung blockiert die Kontoeinstellungen nicht; Breit/Mittel/Schmal und Hell/Dunkel geprüft.

## M11: Eigene Mail-Server und selbst signierte Zertifikate (Issue #284)

Entschieden in der Planung (2026-10-22): Bei Mail-Konten gilt jeder Server, den ein Mensch über
`accounts.connect_mail` einträgt (nur App-Schlüssel), wie er ist: öffentlich, lokales Netz oder dieser Rechner;
kein Schalter. Ein Zertifikat, dem das System nicht vertraut, bestätigt der Mensch einmal; das Konto merkt sich
genau dieses Zertifikat (SHA-256 des Server-Zertifikats); die Rückfrage gibt es überall, bei einem Host mit
öffentlicher Adresse mit deutlicher Warnung (in der Planung entschieden). Reine CalDAV-Konten (`connect_caldav`), die
Dienstsuche und `torro-htmlview` bleiben unverändert. Konzept: [mail.md](../mail.md), Sicherheitsregel
[api.md](../api.md) Fußnote ⁷ und „Was nie nach außen geht“, Punkt 5.

```
M11.1 ─┬─ M11.3 ── M11.4
M11.2 ─┘
```

M11.1 und M11.2 sind unabhängig voneinander. M11.4 braucht nur die Typen aus M11.1 und die Demo-Antworten; sie
läuft nach M11.3, damit die Übersetzungen in `de.rs` nacheinander entstehen.

### M11.1 API und Kontodaten: gemerkte Zertifikate
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-mail/src/account.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torroworkplace/src/audit.rs`, `docs/api-reference.md`; nur das neue Feld (leer) in den Literalen von
  `MailAccountConfig`, `Request::ConnectMail` und `MailView`: `crates/torro-core/src/account.rs`,
  `crates/torroworkplace/src/{core,pim,clients}.rs`, `crates/torroworkplace/src/mail/service.rs`,
  `crates/torroworkplace/src/ui/{accounts,launch,wizard}.rs`, `crates/torro-settings-ui/src/demo.rs`
- **Oberfläche:** M11.4. **API:** `Request::ConnectMail` · `accounts.connect_mail` · Admin (Feld neu).
  **MCP:** Never, wie bisher (Geheimnis, „Was nie nach außen geht“, 5).
- `MailAccountConfig` bekommt `trusted_certificates: Vec<String>` (`#[serde(default, skip_serializing_if =
  "Vec::is_empty")]`, alte `accounts.json` laden unverändert): SHA-256 der Server-Zertifikate, denen ein Mensch
  vertraut hat, in Großbuchstaben-Hex mit `:` (`AB:CD:…`, 95 Zeichen). Doc-Kommentar: gilt für IMAP, SMTP und
  CalDAV des Kontos; höchstens 8, das älteste fällt heraus.
- `Request::ConnectMail` bekommt `trusted_certificates: Vec<String>` (`#[serde(default, skip_serializing_if =
  "Vec::is_empty")]`): Fingerabdrücke, die der Mensch gerade bestätigt hat; der Kern nimmt sie zu den gespeicherten
  hinzu, wenn ein Server des Kontos es bei dieser Anmeldung gezeigt hat (M11.3). Ein Eintrag, der nicht das
  Format von oben hat, ist `error`, bevor ein Server gefragt wird. Doc-Kommentar von `ConnectMail`: Satz „checks that every server's host is on the public
  internet“ und „a host that is not on the public internet (`localhost` included)“ ersetzen durch: jeder Host geht;
  neue Antwort `Response::UntrustedCertificate`.
- Neue Antwort `Response::UntrustedCertificate { service: String, host: String, sha256: String, public: bool }`
  (`result: "untrusted-certificate"`, kebab-case wie alle Antworten, verträglich): `service` ist `IMAP`, `SMTP` oder `CalDAV`; `public`, wenn eine
  Adresse des Hosts öffentlich ist (dann warnt die Oberfläche deutlich); das Passwort ging nicht hinaus, nichts
  wurde gespeichert. Auch Antwort von `accounts.import_torromail_account`.
- `audit::outcome`: `UntrustedCertificate` ergibt `untrusted-certificate`, nicht `ok`.
- `MailView` bekommt `trusted_certificates: Vec<String>` (`#[serde(default)]`); kein Geheimnis.
- Katalog `CONNECT_MAIL` (`api.rs`): Beschreibung um „any server a person typed, also on the local network; a
  certificate the system does not trust is answered with untrusted-certificate and works once the person trusts
  it“ ergänzen. `Access`, `Mcp` bleiben.
- Der Kern reicht das Feld in dieser Aufgabe nur durch (`MailSignIn` in `core.rs`), ohne Verhalten.
- **Abnahme:** Serde-Test: `accounts.json` ohne das Feld lädt, leeres Feld wird nicht geschrieben;
  `ConnectMail` ohne Feld wird gelesen; `TORRO_BLESS=1 cargo test -p torro-ipc` und `docs/api-reference.md`
  committet; `outcome` mit der neuen Antwort; Tabellen-Test der Rechte und `names_follow_the_rules` grün (Client-Schlüssel weiter `forbidden`).

### M11.2 TLS: einem bestätigten Zertifikat vertrauen
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-mail-imap/src/{tls,imap,smtp,lib}.rs`
  (`tls.rs` neu), `crates/torro-mail-imap/Cargo.toml` (`sha2.workspace = true`); nur die `match` auf die neuen
  Varianten in `crates/torroworkplace/src/mail/{send,service,sync}.rs`
- **Oberfläche/API/MCP:** keine; Bibliothek für M11.3.
- Neu `tls.rs`: `Fingerprint([u8; 32])` (SHA-256 des DER des Server-Zertifikats) mit `Display`/`FromStr` im
  Format von M11.1 (Kleinbuchstaben und fehlende `:` beim Lesen erlaubt). `tls_config` zieht aus `imap.rs` hierher
  und wird `pub fn client_config(trust: &Trust) -> Result<(ClientConfig, Seen), …>` (als Wert: reqwest nimmt ihn
  so in `use_preconfigured_tls`; ALPN `h2`, `http/1.1` setzt, wer ihn für HTTP nutzt). `Seen` meldet nach einem
  fehlgeschlagenen Handshake den Fingerabdruck des abgelehnten Zertifikats (`Seen::rejected()`) und, welcher Pin
  angenommen wurde (`Seen::accepted()`); `Session` und `SmtpSubmitter` geben den angenommenen Pin weiter, damit
  M11.3 nur benutzte speichert.
- `Trust` (öffentliches Enum, Änderung freigegeben) bekommt `Pinned { base: Box<Trust>, pins: Vec<Fingerprint> }`
  und `Trust::with_pins(self, Vec<Fingerprint>) -> Trust` (ohne Pins unverändert). Eigener `ServerCertVerifier`:
  passt der Fingerabdruck zu einem Pin, gilt das Zertifikat ohne Prüfung von Name und Ablauf (selbst signierte
  Zertifikate nennen oft einen anderen Namen); sonst entscheidet `base`, bei Ablehnung merkt `Seen` den
  Fingerabdruck. Handshake-Signaturen immer mit dem Provider prüfen (`verify_tls12_signature`,
  `verify_tls13_signature`).
- `ImapError::Untrusted(Fingerprint)`: wenn `Seen` nach dem Handshake etwas meldet (`Session::connect_to`,
  `handshake`). Vorläufig im Kern: `ImapError::Untrusted` wie `Protocol`, `SmtpError::Untrusted` beim Versand wie
  `SmtpError::Tls` („nicht angenommen, nichts gesendet“, nie `unknown`, `send.rs` `smtp_refusal`), in
  `SignInError::smtp`/`imap` wie `Failed`; M11.3 ersetzt das.
- SMTP (lettre nimmt keinen eigenen Verifier): `SmtpSubmitter::trusting(Vec<Fingerprint>)` (additiv). Erst wie
  heute mit dem System; scheitert TLS, eine neue Verbindung mit `dangerous_accept_invalid_certs(true)`, dann
  `peer_certificate()` hashen: Pin → weiter, sonst `abort` und `SmtpError::Untrusted(Fingerprint)`. Kein `AUTH`
  und kein `MAIL FROM` vor diesem Vergleich, auch bei STARTTLS.
- **Abnahme:** Tests mit dem lokalen Test-Server (selbst signiertes Zertifikat, `tests/fixtures/`): ohne Pin
  `Untrusted` mit dem richtigen Fingerabdruck und der Server sah kein Login; mit Pin Anmeldung; mit falschem Pin
  `Untrusted`; dasselbe für SMTP (TLS und STARTTLS); `Fingerprint` liest und schreibt das Format.

### M11.3 Kern: jeder eingetragene Server, gemerkte Zertifikate
- [x] erledigt
- **Modell:** Opus · **Braucht:** M11.1, M11.2 · **Dateien:** `crates/torroworkplace/src/mail/service.rs`,
  `crates/torroworkplace/src/{core,pim}.rs`, `crates/torro-core/src/i18n/de.rs`, `docs/api.md`, `docs/mail.md`
- **Oberfläche:** M11.4. **API/MCP:** M11.1.
- `service.rs`: `Connector::system()` lässt jede Adresse zu; `public_addresses` → `addresses`, `check_public` →
  `check_host`. Auflösen einmal und Verbinden genau mit diesen Adressen bleibt. `MailError::NotPublic` und
  `SignInError::NotPublic` bleiben nur für `Connector::nowhere()` (Texte neutral: „cannot be used“). Modul-Doc
  „Security“ neu: Server kommen nur von Menschen.
- Trust je Konto: `connector.trust.with_pins(…)` aus `config.trusted_certificates` (bei `check_sign_in` die
  gespeicherten plus die der Anfrage) für IMAP (`Connector::connect`, `check_sign_in`) und SMTP (`check_smtp`,
  `MailService::smtp` mit `SmtpSubmitter::trusting`).
- Neu `SignInError::Untrusted(Service, Fingerprint)` aus `ImapError::Untrusted` / `SmtpError::Untrusted`;
  `connect_mail` antwortet damit `Response::UntrustedCertificate` mit dem Host des Dienstes und `public`
  (`discover::is_public` über die aufgelösten Adressen). Gespeichert werden genau die Pins, die bei dieser Anmeldung
  angenommen wurden (`Seen::accepted()`); lief CalDAV nicht mit, bleiben die übrigen gespeicherten dazu (sie können
  zu CalDAV gehören). Höchstens 8. So räumt jedes erneute Verbinden („Server settings…“, „Change password…“)
  nicht mehr benutzte Zertifikate weg, und ein Fingerabdruck, den kein Server zeigte, wird nie gespeichert. `import_torromail_account` läuft über `connect_mail`
  (`MailSignIn` mit leerem Feld) und bekommt dieselbe Antwort, nichts Eigenes.
- Abgleich: `MailError::Untrusted(Fingerprint)`, API-Code `auth`, Meldung ohne Fingerabdruck „The certificate of
  the mail server changed. Check it in Server settings.“; am Konto `Trouble::SignInNeeded`, weil ein Mensch handeln
  muss (wie bei einem fehlenden Passwort). Trotzdem weiter versuchen wie bei `Offline`: TLS scheitert vor dem
  Login, kein Server sperrt das Konto, und ein zurückgesetztes Zertifikat heilt sich so von selbst.
- CalDAV des Mail-Kontos: `Core::check_caldav` ohne Adressprüfung über `CalDavClient::new(http, …)` (nicht
  `pinned`, das kein TLS von außen nimmt); `http` gebaut wie `torro_pim_caldav::http_client()` (User-Agent,
  Timeouts) mit `no_proxy()` und `use_preconfigured_tls(client_config(trust))`, ALPN gesetzt; scheitert die Anmeldung und `Seen` meldet etwas →
  `SignInError::Untrusted(Service::CalDav, …)`. `pim.rs`: `public_http` entfällt; `http_for` gibt für Mail-Konten
  einen Client mit dem Trust des Kontos (je Konto gebaut und im Slot gehalten, ohne Pins der gemeinsame `http`).
  `pinned_public` und `connect_caldav` bleiben.
- Tests umdrehen/ergänzen: `the_core_connects_only_to_public_mail_servers` (service.rs) → private, Loopback und
  ein Name auf eine lokale Adresse gehen; `the_calendars_of_a_mail_account_are_fetched_from_public_addresses_only`
  (pim.rs) → Kalenderteil auf `localhost` gleicht ab; neu im Kern: `connect_mail` gegen einen Server mit selbst
  signiertem Zertifikat → `UntrustedCertificate`, erneut mit Fingerabdruck → verbunden und gespeichert; danach
  anderes Zertifikat → Problem am Konto, kein Login am Server. Bestehende Tests, dass Erkennung
  (`only_public_addresses_pass`, Mail-Erkennung) und Client-Schlüssel (`connect_mail` → `forbidden`) streng
  bleiben, unverändert grün.
- Doku: `docs/api.md` Fußnote ⁷ („Kalender später hinzufügen“: „wie bei M2.7b (öffentlicher Host …)“) und „Was nie
  nach außen geht“, Punkt 5 (Server und vertraute Zertifikate eines Mail-Kontos setzt nur ein Mensch);
  `docs/mail.md` neuer Abschnitt „Eigene Server und Zertifikate“.
- **Abnahme:** die Tests oben; `TORRO_LANG=de cargo test --workspace` grün; `torro-api-review` ohne Befund.

### M11.4 Steuerzentrale: Zertifikat bestätigen
- [x] erledigt
- **Modell:** Opus · **Braucht:** M11.1, M11.3 · **Dateien:** `crates/torroworkplace/src/ui/{wizard,accounts,launch}.rs`,
  `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** „Add account“ › Mail, Kontoseite („Server settings…“, „Change password…“), Übernahme aus
  TorroMail. **API:** M11.1. **MCP:** Never.
- Wizard: auf `Response::UntrustedCertificate` neuer Schritt `Step::MailCertificate`: Titel „Trust this
  certificate?“, Satz „The certificate of {host} ({service}) is not trusted by this computer. Trust it only if this
  is your own server.“; bei `public` darüber eine Warnung mit Icon in `attention` (Farbe nur im Icon): „This server
  is on the public internet. An untrusted certificate there can mean that someone is intercepting the connection.
  Never trust it for a mail provider.“; Fingerabdruck in einer `widgets::group`-Zeile (`typo::meta()`, `.tabular()`, umbrechend);
  „Back“ (zum Passwort) und Primärknopf „Trust certificate“, der `ConnectMail` mit dem Fingerabdruck erneut
  schickt. Dafür behält `Mail` das Passwort, solange die Antwort `UntrustedCertificate` ist; bei jeder anderen
  Antwort, „Back“ oder Schließen wird es gelöscht (`connect_request` nimmt sonst weiter mit `mem::take`).
- Kontoseite: „Server settings“ und „Change password“ zeigen dieselbe Rückfrage im Dialog (Fingerabdruck über dem
  Fuß, „Save“/„Change password“ wird „Trust certificate“); gleiche Passwort-Regel. In der Verbindungsgruppe eine
  Zeile „Trusted certificate“ je Eintrag aus `MailView::trusted_certificates` (gekürzt mit Tooltip).
- TorroMail-Übernahme: `UntrustedCertificate` zeigt an der Zeile „The certificate is not trusted.“ und „Enter
  password“ (Mail-Weg, dort die Rückfrage).
- Demo (`launch.rs`): Fake-`ConnectMail` antwortet `UntrustedCertificate` für einen Host auf `.lan` (`public:
  false`) bzw. `.example` (`public: true`) ohne Fingerabdruck; neue `--open`-Zustände `wizard-mail-certificate` und
  `wizard-mail-certificate-public`.
- **Abnahme:** UI-Tests: Antwort → Schritt mit Host und Fingerabdruck, Warnung nur bei `public`; „Trust certificate“ schickt `ConnectMail`
  mit Passwort und Fingerabdruck; „Back“ löscht das Passwort; Kontoseite ebenso. Sichtprüfung (`torro-design`)
  von `wizard-mail-certificate`, `wizard-mail-certificate-public` und `account-detail` in Schmal, Mittel, Breit, hell und dunkel.

## M12: Anhänge öffnen, speichern und ablegen (Issue #290)

Ein Klick auf den Namen eines Anhangs tut, was die Einstellung sagt (Standard „Open“); das Dreieck daneben bietet
Open, Save to Downloads, Save as… und die **Ablageziele** an: Ordner mit einem Schema für Unterordner und
Dateinamen, Datum aus der Mail oder dem Dateinamen, vorgeschlagen nach Kategorie, Dateityp und Absender. Das
Konzept steht in Issue #290 (Abschnitte „Platzhalter und Regeln“, „Drei Wege“, „Entscheidungen“), Entwurf
[`docs/prototypes/inbox/mail-anhaenge.html`](../prototypes/inbox/mail-anhaenge.html) (Stand 2, Variante V1 für
„Suggest when“).

Entschieden in der Planung (2026-10-24):

- **Ordner setzt nur ein Mensch** (Punkt 9 in „Was nie nach außen geht“, wie `pdf_folder`): `mail.create_save_target`
  nur mit dem App-Schlüssel (`Mcp::Never`, ein neues Ziel setzt immer einen Ordner); `mail.update_save_target` und
  `mail.remove_save_target` sind Admin mit MCP-Tool, ein anderer Ordner als der gespeicherte aber nur vom App-Schlüssel;
  ebenso der Downloads-Ordner (`mail_downloads_folder`). „Save as…“ und „Save all to folder…“ (`to: file` /
  `folder`) nur mit dem App-Schlüssel.
- **Speichern über MCP** folgt der gewöhnlichen Schreibregel (`Write`, „Freigabe vorher“ des Clients), keine eigene
  Freigabepflicht.
- **Ablageziele gelten für alle Konten gleich**; `{account}` steht als Platzhalter bereit.
- Klick-Aktion und Downloads-Ordner sind Einstellungen (`settings.set_setting`); die Ziele haben eigene Befehle und
  eine eigene Datei `<data>/mail/save-targets.json` (nicht in `settings.json`, damit `settings.update_settings` sie
  nie mitschreibt).
- Die Schema-Auflösung und die Vorschlagsregel sind reine Funktionen in `torro-mail` (`save_target.rs`): Der Kern
  rechnet damit jeden echten Pfad, die Einstellungsseite nur die Live-Vorschau an einem festen Beispiel.

```
M12.1 ─┬─ M12.2 ── M12.3 ── M12.4
       └─ M12.5 ─────────── M12.6 ── M12.7
```

Reihenfolge der Umsetzung: M12.1 bis M12.7 nacheinander (Übersetzungen in `de.rs` je Aufgabe). M12.6 braucht M12.3
nur für den echten Lauf; ihre Tests gehen über Demo und Fakes.

### M12.1 API-Gerüst: Ablageziele, Speichern, Einstellungen
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-mail/src/{lib,save_target}.rs` (neu),
  `crates/torro-ipc/src/{lib,api,mail}.rs`, `crates/torro-core/src/settings.rs`,
  `crates/torroworkplace/src/{settings_service,core,pim,audit,daemon,approvals}.rs`, `docs/api-reference.md`,
  `docs/api.md`
- **Oberfläche:** M12.6, M12.7. **API/MCP:** die Tabelle unten. MCP-Tools entstehen aus dem Katalog, die bequeme
  Eingabe von `mail_save_attachments` in M12.4.
- `torro-mail` neu `save_target.rs` (nur Typen, Serde camelCase wie die Nachbarn, `JsonSchema`):
  `SaveTargetSpec { name, folder, subfolders, fileName, dateFrom: DateSource, suggest: SuggestWhen, onConflict:
  OnConflict }`, `SaveTarget { id, #[serde(flatten)] spec }`, `DateSource { Mail, FileName }` („File name, else
  mail“), `OnConflict { KeepBoth, SkipIdentical }`, `SuggestWhen { categories: Vec<Category>, kinds:
  Vec<FileKind>, senders: Vec<String> }` (alle `#[serde(default)]`), `FileKind { Pdf, Image, Office }`. Standardwerte
  für ein neues Ziel: `subfolders` `{year}/{month}`, `fileName` `{name}.{ext}`, `Mail`, `KeepBoth`.
- `torro-ipc` (`mail.rs`): `SaveTo` extern getaggt: `"downloads"` | `{"target": id}` | `{"file": path}` (genau ein
  Anhang, ersetzt eine Datei dort, der Dialog hat gefragt) | `{"folder": path}` (bestehender Ordner, Konflikt wie
  `KeepBoth`); `AttachmentPlan { part, fileName, places: Vec<PlannedPlace { target, folders: Vec<String>, file }>,
  suggested: Vec<String>, sure: Option<String> }`; `SavedFile { part, path }`, `FailedFile { part, name, code,
  message }`.
- Requests (`lib.rs`, Doc-Kommentare mit Antwort und Fehlercodes) und Katalog (`api.rs`):

  | Request | Name · Zugriff | MCP | Antwort |
  |---|---|---|---|
  | `ListSaveTargets { id: Option<String> }` | `mail.list_save_targets` · Read | Tool | `SaveTargets { targets, downloads, files }` (`files` nur mit `id`: je Anhang ohne `inline` ein `AttachmentPlan`) |
  | `CreateSaveTarget { target: SaveTargetSpec }` | `mail.create_save_target` · Admin | Never (Punkt 9) | `SaveTarget { target }` |
  | `UpdateSaveTarget { id, target: SaveTargetSpec }` | `mail.update_save_target` · Admin | Tool | `SaveTarget { target }` |
  | `RemoveSaveTarget { id }` | `mail.remove_save_target` · Admin | Tool | `SaveTargets` ohne `files` |
  | `SaveMailAttachments { id, parts: Vec<String>, to: SaveTo }` | `mail.save_attachments` · Write | Tool | `SavedAttachments { saved, skipped, failed }` |

  `result`-Werte kebab-case (`save-targets`, `save-target`, `saved-attachments`). Leere `parts` = alle Anhänge ohne
  `inline`. Beschreibungen: Ziele gelten für alle Konten, es wird nie überschrieben, `skipped` heißt „gleicher Inhalt
  liegt schon dort“, Pfade setzt nur ein Mensch, Dateinamen sind `untrusted`; bei `mail.update_save_target`: `folder`
  muss der Wert aus `mail_list_save_targets` sein, ein anderer gibt `forbidden`; bei `mail.remove_save_target`: die
  Klick-Aktion springt auf `open` zurück, wenn sie das Ziel nannte.
- `Request::kept_for_people`: `SaveMailAttachments` mit `to` `file` oder `folder` → `true`, Doc-Kommentar der Funktion
  ergänzen. `names_follow_the_rules`: `mail.create_save_target` in `kept_back` mit Kommentar „Point 9“. Im selben
  Schritt `docs/api.md`, „Was nie nach außen geht“, Punkt 9 um Ablageziele (Ordner), Downloads-Ordner und `to:
  file|folder` ergänzen, damit Katalog und Liste nie auseinanderlaufen.
- `torro-core` `settings.rs`: `Settings::mail_attachments: MailAttachments { click: AttachmentClick, downloads:
  Option<String> }` (`#[serde(default)]`); `AttachmentClick` `"open"` (Standard) | `"downloads"` | `"save_as"` |
  `"suggested"` | `{"target": id}`. `SettingKey::MailAttachmentClick` (`mail_attachment_click`) und
  `SettingKey::MailDownloadsFolder` (`mail_downloads_folder`, absoluter Pfad oder `null` für den Downloads-Ordner des
  Systems; entfernt ein Client ihn, gilt wieder der des Systems) mit Doc-Kommentaren wie `PdfFolder`.
- `settings_service.rs`: `Change::MailAttachmentClick`, `Change::MailDownloadsFolder` (Lesen wie `PdfFolder`,
  `limited_for_clients` mit `kept_or_removed`). `settings.rs`: `MailAttachments::needs_a_person(&new)` und
  `MailAttachments::from_client(&sent)` nach dem Vorbild von `Tools`; `from_client` neben `settings.tools.from_client`
  im Client-Zweig von `update_settings` (`settings_service.rs`). `core.rs` `switches_on_for_people`: Arm
  `SetSetting MailDownloadsFolder` mit anderem Ordner als gespeichert, im Arm `UpdateSettings`
  `now.mail_attachments.needs_a_person(…)`.
- Kern vorläufig: die fünf Requests in `core.rs` an einen Arm, der `unsupported` antwortet (M12.3 ersetzt ihn);
  `pim.rs` wie `GetMailAttachment`; `audit.rs` nur Nachrichten-Kennung und Ziel-Kennung, keine Datei- und
  Zielnamen, keine Pfade (`to.file`/`to.folder`, aufgelöste Orte), keine Schemas, keine Absender; `daemon.rs`
  Kurztexte („save {n} mail attachments“, „list save targets“ …); `approvals.rs`: Zusammenfassung mit Platzhalter,
  M12.3 setzt die Namen ein.
- **Abnahme:** Serde-Rundtrip aller neuen Typen und Requests (auch `SaveTo` als Zeichenkette/Objekt);
  `settings.json` ohne `mail_attachments` lädt; Test, dass ein Client mit Admin in `mail` für `to: file|folder`
  `forbidden` bekommt und für `downloads`/`target` nicht; `only_people`-Test für `mail_downloads_folder` (wie
  `pdf_folder`, `torro-ipc` und Kern); Tabellen-Test der Rechte und `names_follow_the_rules` grün;
  `TORRO_BLESS=1 cargo test -p torro-ipc`, `docs/api-reference.md` committet.

### M12.2 Schema auflösen und Vorschlag
- [x] erledigt
- **Modell:** Opus · **Braucht:** M12.1 · **Dateien:** `crates/torro-mail/src/save_target.rs`
- **Oberfläche/API/MCP:** keine; Bibliothek für M12.3 und M12.7.
- Reine Funktionen ohne Dateizugriff:
  - `FileFacts { mail_date: NaiveDate, file_name: Option<String>, content_type, sender_name, sender_address,
    account, subject, month_name: String }` – den Monatsnamen gibt der Aufrufer (in der Sprache der App), weil
    `torro-mail` kein `tr` kennt.
  - `date_in_name(name) -> Option<NaiveDate>`: `2026-03-28`, `28.03.2026`, `20260328`, nur gültige Daten, das erste
    Vorkommen; Ziffern direkt davor oder danach verhindern den Treffer (`120260328` nicht).
  - `clean_segment(text) -> String`: kein `/` und `\` (zu `_`), keine Steuer- und unsichtbaren Zeichen, Zeichen, die
    Windows ablehnt, zu `_`, keine führenden Punkte und Leerzeichen (nie `.`/`..`), keine Namen, die Windows
    reserviert (`CON`, `NUL`, `COM1` … → `_CON`), höchstens 120 Bytes; leer bleibt leer.
  - `resolve(spec, facts) -> Result<Resolved { folders: Vec<String>, file: String }, SchemaError>`: Platzhalter
    `{year} {month} {day} {month_name} {quarter} {date} {sender} {sender_domain} {account} {subject} {name} {ext}`
    (Werte wie in der Tabelle des Issues; `{subject}` höchstens 60 Zeichen; `{name}` ohne, `{ext}` die Endung ohne
    Punkt, kleingeschrieben); unbekannter Platzhalter oder `{` ohne `}` → `SchemaError::Unknown(name)`. Ordner nur
    durch `/` im Schema, jedes Stück für sich bereinigt, leere Stücke entfallen; der Dateiname behält beim Kürzen die
    Endung, fehlt `{ext}` im Wert, entfällt der Punkt am Ende; leerer Dateiname → bereinigter Originalname, sonst
    `attachment`.
  - `FileKind::of(content_type, name)`: PDF; Bilder (`image/*`, png jpg jpeg gif heic webp); Office (doc(x) xls(x)
    ppt(x) odt ods odp und ihre MIME-Typen).
  - `suggestion(when, category: Option<MailCategory>, kind, sender_address) -> Option<Suggestion { sure: bool }>`:
    ohne Bedingung nie; jede gesetzte Art muss passen, innerhalb einer Art reicht ein Treffer; Absender passt auf
    die ganze Adresse oder auf die Domain bzw. eine darunter, ohne Rücksicht auf Groß- und Kleinschreibung; `sure` nur,
    wenn keine Kategorie-Bedingung gesetzt ist oder die Kategorie nicht `is_suggestion()` ist.
- **Abnahme:** Tests für jeden Platzhalter, beide Datumsquellen (Datum im Namen in allen drei Formen, kein Datum →
  Mail), feindliche Werte (`../`, `/etc`, Steuerzeichen, `CON`, sehr lange Werte) bleiben ein Stück unter dem Ordner,
  unbekannter Platzhalter, Vorschlag nach Kategorie, Typ, Absender (Adresse und Domain), ohne Bedingung nie, vermutete
  Kategorie → `sure: false`.

### M12.3 Kern: Ziele speichern, Anhänge ablegen
- [x] erledigt
- **Modell:** Opus · **Braucht:** M12.2 · **Dateien:** `crates/torroworkplace/src/mail/{mod,save}.rs` (`save.rs` neu),
  `crates/torroworkplace/src/mail/{read,service}.rs`, `crates/torroworkplace/src/{core,approvals}.rs`,
  `crates/torro-core/src/i18n/de.rs`, `docs/mail.md`
- **Oberfläche:** M12.6, M12.7. **API/MCP:** M12.1.
- `save.rs`: Datei `<data>/mail/save-targets.json` (0600, schreiben über Temp-Datei und Umbenennen wie die Nachbarn),
  Kennungen zufällig (`random_hex`). Prüfen beim Anlegen und Ändern (sonst `error`): höchstens 50 Ziele, Name 1–60
  Zeichen, eine Zeile, eindeutig ohne Rücksicht auf Groß- und Kleinschreibung; `folder` absolut, höchstens wie
  `MAX_PATH_BYTES`; Schemas höchstens 200 Zeichen, eine Zeile, `resolve` mit einem Beispiel ohne `SchemaError`;
  höchstens 20 Absender à 200 Zeichen; Kategorien aus dem Katalog. `not-found` für eine unbekannte Kennung.
  `update_by_client(id, spec)` setzt `folder` unter derselben Sperre wie das Schreiben auf den gespeicherten Wert
  (Gedanke von `kept_or_removed`): Ändert ein Mensch den Ordner zwischen der Prüfung im Kern (oder der Freigabe) und dem
  Schreiben, bleibt seiner. Der Kern ruft es im Client-Zweig, `update` nur für den App-Schlüssel.
- `mail.list_save_targets`: Ziele, `downloads` (Einstellung, sonst `dirs::download_dir()`, sonst `<home>/Downloads`)
  und mit `id` je Anhang der Plan: `FileFacts` aus der gespeicherten Nachricht (Datum, Absender, Betreff, Konto
  `display_name` sonst Adresse, Monatsname über `tr`), Orte je Ziel, `suggested` (passende Ziele in Listenordnung),
  `sure` (erstes mit `sure`). Nachrichten nur über `mail.stores(reach)` wie `get_attachment`. Das Datum der Mail
  ist ihr Tag in der lokalen Zeitzone des Rechners (`Local`), nicht in UTC. Kennt der Speicher die Anhänge noch nicht
  (Text nicht geladen), bleibt `files` leer; die Oberfläche zeigt dann die Ziele ohne Pfad.
- `mail.save_attachments`: Nachricht über `mail.stores(reach)` und `locate` finden wie `get_attachment` (ein Konto
  außerhalb der Reichweite ist `not-found`, bevor etwas gelesen wird; nach einer Freigabe über `mail_to_approve`), dann
  einmal holen (`fetch_raw`), jeden Teil mit `mime::attachment`; je Datei höchstens
  `ATTACHMENT_MAX_BYTES`, sonst `failed`. Ziel:
  - `downloads`: bereinigter Name in den Downloads-Ordner, Konflikt wie `KeepBoth`;
  - `target`: Grundordner muss bestehen (sonst Fehler `not-found` „The folder of {target} is missing.“ für den
    ganzen Aufruf). Vor dem Anlegen den tiefsten vorhandenen Vorfahren des Zielordners `canonicalize`n und prüfen,
    dass er unter dem kanonischen Grundordner liegt, dann die Unterordner einzeln anlegen, danach den fertigen Ordner
    noch einmal prüfen (sonst `failed`; so entstehen über einen Symlink keine Ordner außerhalb); jede Datei nach ihrem
    eigenen Datum;
  - `file`: wie `tools.save_file_as` (absolut, Elternordner besteht, kein Ordner und kein Symlink als Ziel, ersetzt);
  - `folder`: bestehender Ordner, `KeepBoth`.
  Nie überschreiben außer bei `file`: Schreiben mit `create_new`; `KeepBoth` versucht `Name (2).ext` bis `(99)`;
  `SkipIdentical` vergleicht den Inhalt einer vorhandenen Datei gleichen Namens (gleich → `skipped`, sonst wie
  `KeepBoth`). Scheitert eine Datei, laufen die übrigen weiter (`failed` mit Code und Meldung ohne Pfad).
  `io::ErrorKind::PermissionDenied` (macOS fragt den Dienst vor dem ersten Schreiben in Downloads, Dokumente und
  Schreibtisch) ergibt Code `forbidden` und die Meldung „Torro Workplace may not write to this folder.“, unter macOS
  ergänzt um „Allow it in System Settings › Privacy & Security › Files and Folders.“ (`#[cfg(target_os = "macos")]`).
- `core.rs`: Arm aus M12.1 durch die Weiterleitung an `save.rs` ersetzen (geboxt wie die Lese-Requests);
  `switches_on_for_people`: `UpdateSaveTarget` mit anderem `folder` als gespeichert. `RemoveSaveTarget` setzt
  `mail_attachment_click` auf `open` zurück, wenn es dieses Ziel nannte.
- `approvals.rs`/`core.rs`: Namen aus dem Speicher des Kerns (`Facts` mit Nachschlage-Regel über die Kennung), nie
  aus dem Request. Für `SaveMailAttachments` füllt der Kern `MailFacts` (Betreff aus dem Speicher, Konto) und zählt
  `{n}` selbst (bei leeren `parts` die gespeicherten Anhänge ohne `inline`); die Freigabe wird an die Nachricht gebunden
  wie bei `CreateTodoFromMail` (`mail_fingerprint`). Texte: „Save {n} attachments of “{subject}” to {target}“ bzw.
  „… to Downloads“; „Change save target {name}“ mit dem gespeicherten Namen, ein neuer Name nur als „(renamed to
  {new})“; „Remove save target {name}“.
- Doku: `docs/mail.md` Tabelle der Funktionen um die fünf Befehle und kurzer Abschnitt „Anhänge ablegen“ (Verweis auf Issue #290).
- **Abnahme:** Handler-Tests mit Temp-Verzeichnissen (`TORRO_DATA_HOME`, Downloads-Ordner gesetzt): Bereinigung,
  Pfad bleibt im Grundordner (auch mit Symlink im Grundordner), nie überschreiben (`KeepBoth` zählt hoch),
  „Skip if identical“, Größenlimit, Teilerfolg bei „Save all“, fehlender Grundordner, `to: file|folder` von einem
  Client `forbidden`, `update_save_target` mit neuem Ordner von einem Client `forbidden` und mit dem gespeicherten
  erlaubt, Ordner vom Menschen geändert zwischen Prüfung und Schreiben bleibt, Client ohne Zugriff auf das Konto
  bekommt bei `list_save_targets` mit `id` und bei `save_attachments` `not-found`, Freigabe-Text mit Betreff, Zahl und
  Zielname aus dem Speicher, Plan mit `suggested`/`sure`, Entfernen setzt die Klick-Aktion zurück; `TORRO_LANG=de cargo test
  --workspace` grün.

### M12.4 MCP: `mail_save_attachments` ohne Pfad
- [x] erledigt
- **Modell:** Haiku · **Braucht:** M12.3 · **Dateien:** `crates/torroworkplace/src/mcp.rs`
- **Oberfläche/API:** M12.1. **MCP:** `mail_save_attachments`, `mail_list_save_targets`, `mail_update_save_target`,
  `mail_remove_save_target`.
- Bequeme Eingabe für `mail.save_attachments` (`easy_input_schema`, `request_for`): `id`, `parts` (optional), `to`
  = `"downloads"` oder die Kennung eines Ziels aus `mail_list_save_targets`; daraus `SaveTo::Downloads` bzw.
  `SaveTo::Target`. Kein Feld für einen Pfad. Die übrigen drei Tools kommen unverändert aus dem Katalog.
- **Abnahme:** Test, dass das Tool `downloads` und eine Kennung in den richtigen Request übersetzt und unbekannte
  Felder (`file`, `folder`, `path`) ablehnt; `mail_create_save_target` steht nicht in `tools()`;
  Vertragstest „Tool ↔ Katalog“ grün.

### M12.5 `torro-gui`: geteilter Chip, Anhangblock, Menüeinträge mit Pfad
- [x] erledigt
- **Modell:** Haiku · **Braucht:** M12.1 · **Dateien:** `crates/torro-gui/src/{widgets,icon}.rs`,
  `crates/torro-gui/assets/icons/{download,save}.svg` (neu), `crates/torro-gui/examples/gallery.rs`
- **Oberfläche:** Bausteine für M12.6. **API/MCP:** keine.
- `widgets::split_file_chip(name, size_text, on_open, on_menu, anchor: Option<&Anchor>)`: wie `file_chip`, rechts
  nach einer Haarlinie (`LINE_STRONG`) ein Knopf mit `ChevronDown` (14), Tooltip „More ways to open or save“;
  beide Teile mit eigenem Hover, gemeinsamer Rahmen; das Dreieck registriert `anchor` (`anchored_if`).
  `widgets::split_action(icon, label, on_press, on_menu, anchor)`: dieselbe Teilung ohne Fläche und Rand (ghost) für
  „Save all ▾“.
- `widgets::file_block(head, content)`: Rahmen `LINE_STRONG`, Radius `radius::sm()`, Kopf als Band mit `LINE`
  darunter (Büroklammer, Titel `typo::body_strong()`, Meta `typo::meta()` in `TEXT_3`, rechts frei für eine
  Aktion), darunter der Inhalt mit `space::s()` Abstand.
- `MenuItem::detail(text)` (zweite Zeile `typo::meta()` in `TEXT_3`, einzeilig mit „…“; der Eintrag wird zwei Zeilen
  hoch) und `MenuItem::tag(label, sparkles)` (Wort `typo::caption()` in `TEXT_3` vor dem Trail, mit `Icon::Sparkles`
  in `accent` davor bei `sparkles`; höchstens zwei).
- Neue Icons `Icon::Download` und `Icon::Save` (Lucide 0.469.0, `stroke-width="1.75"`, Eintrag in `Icon::ALL`).
- Galerie `--mailbar`: Anhangblock mit drei geteilten Chips und „Save all ▾“, ein Menü mit Detail und beiden Tags.
- **Abnahme:** Doc-Kommentare mit Beispiel; `scripts/design-check.sh` grün; Galerie in Standard, Compact, Console,
  hell und dunkel angesehen.

### M12.6 Leser, Panel und Kontextmenü: Dreieck-Menü, Klick-Aktion, Speichern
- [x] erledigt
- **Modell:** Opus · **Braucht:** M12.3, M12.5 · **Dateien:** `crates/torroinbox/src/{mail,app,detail}.rs`,
  `crates/torro-ipc/src/inbox.rs`, `crates/torro-inbox/src/demo.rs`, `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** Leser, Panel, Kontextmenü der Liste. **API:** `mail.list_save_targets`, `mail.save_attachments`.
  **MCP:** M12.4.
- `MailSource`: `save_targets(id)` und `save_attachments(id, parts, to)`; umsetzen in `InboxClient`, Demo (zwei Ziele
  „Invoices“ `{year}/{month}` mit Kategorie Invoice und „Contracts“; Speichern antwortet ohne Datei) und den Fakes der
  Tests.
- Beim Öffnen einer Mail mit Anhängen `save_targets(Some(id))` laden und am Lesezustand halten; die Klick-Aktion aus
  `Settings::mail_attachments` mit `mail_view` laden (`fetch_look`).
- Leser: `attachment_line` wird `widgets::file_block` mit Kopf „Attachments“ und „{n} files · {size}“, rechts ab zwei
  Anhängen „Save all ▾“; Chips `split_file_chip`. Panel (`panel_foot`, `attachment_row`): Dreieck an jeder Zeile,
  „Save all ▾“ ab zwei.
- Klick auf den Namen (`Message::OpenAttachment` wird zur Aktion nach Einstellung): `open` wie bisher; `downloads`,
  `target` → `save_attachments`; `save_as` → `rfd` Speichern-Dialog mit dem Namen aus `AttachmentPlan::file_name`,
  dann `to: file`; `suggested` → `sure`, sonst Öffnen. Ein Ziel, das nicht mehr in der Liste steht, gilt als Öffnen.
  Tooltip des Chips „{action}: {name}“. „Save all“: Hauptklick nach Einstellung, bei `open`/`suggested` Downloads.
- Menü am Dreieck (`Menu::Attachment(part)`, Anker in `detail::Anchors`): Open (`ArrowUpRight`), Save to Downloads
  (`Download`), Save as… (`Save`); Abschnitt „Save to“ mit den Zielen (`Folder`), vorgeschlagene zuerst, Detail „Invoices › 2026 › 03“, Tag „Suggested“ (Sparkles nur wenn
  nicht `sure`); Tag „Click“ an der Klick-Aktion; „Manage save targets…“ öffnet die Einstellungen bei `attachments`.
  Für „Save all“ dieselbe Liste mit „Save all to Downloads“, „Save all to folder…“ (`rfd` Ordner-Dialog, `to:
  folder`) und Detail „by each file's date“, ohne Open.
- Rail: Gruppe File bekommt „Save attachments ({n})“ (Icon `Download`), nur bei Anhängen; ein Klick öffnet das Menü
  von „Save all“ am Item; im Kontextmenü der Liste (`ContextAction`) öffnet es dasselbe Menü an der Zeile.
- Nach dem Speichern Hinweis „Saved to {where}“ bzw. „{n} attachments saved to {target}“, bei `failed` „{n} saved,
  {name} failed“; `Hint` bekommt neben `undo` die Aktion „Show in folder“ (öffnet den Ordner der ersten Datei wie
  `open_attachment` für Programme). Abbrechen im Dialog: nichts, keine Meldung.
- Demo: `--open attachment-menu`, `save-all-menu`, `attachment-saved`.
- **Abnahme:** UI-Tests: Klick nach jeder Einstellung (auch vermutete Kategorie → Öffnen), Menü mit Click/Suggested,
  Save all ab zwei Anhängen, Kontextmenü führt zum Menü, Abbrechen sendet nichts, Hinweis mit „Show in folder“;
  Sichtprüfung (`torro-design`) der drei Zustände in Schmal, Mittel, Breit, hell und dunkel, Standard und Console.

### M12.7 Einstellungen › Mail › Attachments
- [x] erledigt
- **Modell:** Opus · **Braucht:** M12.2, M12.6 · **Dateien:** `crates/torro-settings-ui/src/{lib,mail,demo}.rs`,
  `crates/torro-settings-ui/src/mail/attachments.rs` (neu), `crates/torroinbox/src/app.rs`,
  `crates/torro-core/src/i18n/de.rs`, `docs/settings.md`
- **Oberfläche:** Einstellungen › Mail › Attachments. **API:** `settings.set_setting` (`mail_attachment_click`,
  `mail_downloads_folder`), `mail.list_save_targets`, `mail.create_save_target`, `mail.update_save_target`,
  `mail.remove_save_target`. **MCP:** M12.4 (Anlegen Never).
- Abschnitt `Section { id: "attachments", icon: Icon::Paperclip, label: "Attachments" }` nach „Reading“; `wanted()`
  um `ListSaveTargets { id: None }`; `Data::save_targets` und `Data::downloads`.
- Seite (C): Gruppe „Clicking an attachment“ mit „Click on an attachment“ (Auswahl: Open, Save to Downloads, Save as…,
  je Ziel „Save to {name}“, „Suggested target, else open“; Satz „The triangle beside it always offers every way.“) und
  „Downloads folder“ mit „Change…“; Gruppe „Save targets“ mit erklärendem Satz, je Ziel eine Zeile (Name, Pfad und
  Schema in `TEXT_3`), „Add save target“.
- Editor (D) als Dialog (`parts::dialog`), Bezeichnung links, Feld rechts, drei Abschnitte: Name, Folder
  („Choose…“); Subfolders und File name je mit „Insert ▾“ (Vorlagen und Platzhalter mit Beispielwert, eingefügt an
  der Cursorstelle) und Satz „A “/” starts a new folder.“, „Date from“ (Segmente „Mail date“ / „File name, else
  mail“); „Suggest when“ als Bedingungen mit ✕ und „+ Add condition“ (Menü: Kategorien der Gruppe PaperTrail und
  weitere aus dem Katalog, Dateitypen, Absender als Eingabe), „If file exists“ (Segmente „Keep both“ / „Skip if
  identical“); unten die Live-Vorschau mit `resolve` am festen Beispiel des Entwurfs
  (`Rechnung_2026-03-28_4711.pdf` von Stadtwerke Bonn, 2 Apr 2026); Fuß „Delete target“ (danger, mit Rückfrage),
  „Cancel“, „Save“. Fehler (Name leer, Schema mit unbekanntem Platzhalter) erst nach dem ersten Speicherversuch.
- Ordner wählen: `AppAction::PickFolder(FolderFor)` (Downloads oder Editor) wie in `assistant/tools.rs`; das
  Mail-Fenster öffnet `rfd` und gibt den Pfad zurück (`settings_outcome` statt `Outcome::App(())`).
- Demo-Antworten für die vier Befehle; `--settings attachments`.
- `docs/settings.md`: Zeile „Mail“ um „Attachments“ ergänzen.
- **Abnahme:** Seiten-Tests: Auswahl schickt `SetSetting MailAttachmentClick`, „Change…“ schickt
  `MailDownloadsFolder` mit dem gewählten Pfad, Editor schickt `CreateSaveTarget` bzw. `UpdateSaveTarget` mit allen
  Feldern, „Insert“ fügt an der Cursorstelle ein, Vorschau folgt den Feldern, Fehler erst nach Speichern; Sichtprüfung
  (`torro-design`) von Seite und Editor in Schmal, Mittel, Breit, hell und dunkel; `torro-design-review` und
  `torro-api-review` ohne Befund.
