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
- [ ] erledigt
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
- **Modell:** Sonnet · **Braucht:** M7.2 · **Dateien:** `crates/torroworkplace/src/{account,core,mcp}.rs`,
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
