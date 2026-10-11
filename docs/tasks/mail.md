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
- [ ] erledigt
- **Modell:** Opus (Agent `nextcloud-expert` geeignet) · **Braucht:** M1.1 · **Dateien:**
  `crates/torro-core/src/account.rs`, `crates/torro-pim-caldav/src/{lib,client}.rs` und neu
  `discover.rs`, `crates/torroworkplace/src/{account,pim,core,mcp}.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `docs/api-reference.md`
- **Konto:** `AccountKind::CalDav` mit Server-URL (die gefundene DAV-Basis), Anmeldename und
  Anzeigename (Anbieter bzw. Domain, z. B. „mailbox.org“); Kennung `caldav:<login>@<host>` (nie mit
  `client-` beginnend); Passwort bzw. App-Passwort im Schlüsselbund unter der Kennung wie bei Nextcloud.
- **Dienstsuche** (`discover.rs`, ohne Anmeldung): aus einer E-Mail-Adresse bzw. Domain der Reihe nach
  1. DNS SRV `_caldavs._tcp.<domain>` (+ TXT `path=`), RFC 6764;
  2. `https://<domain>/.well-known/caldav` und `https://caldav.<domain>/.well-known/caldav` mit
     Weiterleitungen (höchstens 5, nur https);
  3. eine kleine Tabelle bekannter Anbieter (mailbox.org, Posteo, Fastmail, iCloud, Zoho; Gmail und
     Outlook/Hotmail/Microsoft-Domains als „nicht unterstützt“ mit Grund).
  Ergebnis: Kandidaten-URL(s) oder „nicht gefunden“. Mit Anmeldung prüft `CalDavClient` dann
  `current-user-principal` und meldet, ob es **Kalender** (VEVENT) und **Aufgaben** (VTODO) gibt.
- **Sync** wie Nextcloud über M1.1 (eigener Zustand je Konto); Nextcloud-Eigenes (Remote Wipe,
  Login Flow, Platzhalternamen) nur für Nextcloud-Konten. 401/403 → Problem „Password was refused“ am Konto.
- **API:**
  - `Request::DiscoverServices { address }` · `accounts.discover_services` · Zugriff Read – sucht ohne
    Anmeldung, liefert Anbieter, gefundene URL, „unsupported“ mit Grund.
  - `Request::ConnectCalDav { address, server: Option<String>, login_name, password }` ·
    `accounts.connect_caldav` · Zugriff Admin – meldet an, prüft, speichert das Passwort nur im
    Schlüsselbund, legt das Konto an und gleicht ab. Antwort ohne Passwort. Fehlercodes `auth`,
    `not-found` (kein CalDAV), `unsupported`, `offline`.
- **MCP:** `accounts_discover_services` ja. `accounts_connect_caldav` **Never** – das Passwort müsste
  durch den Agenten gehen; Agenten verwalten nie Konten mit Geheimnissen („Was nie nach außen geht“).
- **Abnahme:** Tests ohne Netz: Suche gegen einen lokalen Fake-Resolver und `httpmock` (SRV, well-known
  mit Weiterleitung, Tabelle, nicht unterstützt), Anmelden mit falschem Passwort (`auth`), zwei Konten
  (Nextcloud + CalDAV) synchronisieren getrennt; Antworten enthalten nie das Passwort; API-Referenz neu
  erzeugt; `torro-api-review` ohne Befund.

### M7.2 Steuerzentrale: Mail-Anbieter im Assistenten
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** M7.1, M1.2 · **Dateien:** `crates/torroworkplace/src/ui/{wizard,accounts}.rs`
- **Oberfläche:** Im Assistenten neben „Nextcloud“ der Anbieter „Mail provider (calendar and tasks)“.
  Schritt 1: E-Mail-Adresse → `accounts.discover_services`; gefundener Anbieter mit Logo-freiem Namen,
  „Server“ nur aufklappbar unter „Advanced“. Bei „nicht unterstützt“ der Grund und nichts weiter.
  Schritt 2: Passwort-Feld mit Hinweis, wo es nötig ist: „Use an app password“ mit Link-Text für iCloud
  und Fastmail. Schritt 3: gefundene Dienste als Häkchen (Calendars, Tasks), vorausgewählt; „Mail“
  ausgegraut mit „Coming later“. Fehler `auth` am Passwort-Feld, erst nach dem ersten Versuch.
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
- **Modell:** Sonnet · **Braucht:** M7.2 · **Dateien:** `crates/torroworkplace/src/{account,core}.rs`,
  `crates/torroworkplace/src/ui/wizard.rs`, `crates/torro-ipc/src/{lib,api}.rs`
- **Oberfläche:** Gibt es TorroMail-Konten, bietet der Assistent oben „Use accounts from TorroMail?“ an:
  Liste mit Adresse und gefundenem CalDAV; übernommen werden Adresse, Server, Benutzer. Das Passwort holt
  der Kern **nach Zustimmung** aus dem Schlüsselbund-Eintrag `TorroMail/<id>` und legt es unter
  `torroworkplace` ab (siehe `mail.md`, Abschnitt TorroMail).
- **API:** `Request::TorroMailAccounts` · `accounts.list_torromail_accounts` · Read (ohne Geheimnisse);
  `Request::ImportTorroMailAccount { id }` · `accounts.import_torromail_account` · Admin.
- **MCP:** `accounts_list_torromail_accounts` ja; Import **Never** (bewegt ein Geheimnis).
- **Abnahme:** Tests mit Fake-Schlüsselbund; Import ohne Zustimmung unmöglich; `torro-api-review`.
