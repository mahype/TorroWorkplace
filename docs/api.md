# API und MCP: jede Funktion dreimal

Stand: 2026-10-11. **Freigegeben** (Entscheidungen unten). Aufgaben: [tasks/api-mcp.md](tasks/api-mcp.md).

## Die Regel

Jede Funktion der Suite gibt es auf **drei Wegen**:

| Weg | Wer | Wie |
|---|---|---|
| **Oberfläche** | Menschen | Steuerzentrale, Apps, Tray-Menü, `ctl` |
| **API** | Skripte, Leisten, andere Programme | versioniertes Protokoll des Kerns, mit Schlüssel |
| **MCP** | KI-Agenten | `torroworkplace mcp`, ein Tool je Funktion, baut **auf der API** auf |

Es gibt die Logik nur einmal, im Dienst. Oberfläche, API und MCP sind drei Türen zu denselben
Befehlen. Was die Oberfläche kann, kann die API, und was die API kann, kann MCP. Ausnahmen gibt
es nur aus Sicherheitsgründen, und sie stehen vollständig unten in „Was nie nach außen geht“.

Daraus folgt für die Planung: **Jede neue Funktion wird mit allen drei Wegen geplant.** Eine
Aufgabenliste nennt zu jeder Funktion Oberfläche, API-Befehl und MCP-Tool, oder sie begründet,
warum ein Weg fehlt (siehe „Arbeitsweise“).

## Was es schon gibt

Die Grundlage steht: Alles, was Steuerzentrale, Apps, Tray und `ctl` tun, geht heute schon als
`torro_ipc::Request` über `core.sock` an den Dienst (`crates/torro-ipc/src/lib.rs`). Die Seiten
der Steuerzentrale schicken nur Requests (`Action::Send`), sie lesen und schreiben keine Dateien.
**Die API ist dieser Request-Satz**, nur festgeschrieben, versioniert, beschrieben und mit
Anmeldung. Es entsteht keine zweite Schnittstelle neben dem IPC.

## Aufbau

```
 Steuerzentrale · Apps · Tray · ctl        Skripte · Leisten      KI-Agent (Claude Code, Codex …)
              |                                   |                          |  stdio
              |  App-Schlüssel                    |  Client-Schlüssel        |
              |                                   |                 torroworkplace mcp
              |                                   |                 (TORRO_TOKEN = Client-Schlüssel)
              +--------------- core.sock (API v1) +--------------------------+
                                     |       (später optional: http://127.0.0.1, gleiche API)
                          torroworkplace daemon
              Anmeldung → Rechte → Freigabe (wenn nötig) → Ausführung → Protokoll
```

- Der MCP-Server ist ein **dünner Übersetzer**: Er nimmt einen Tool-Aufruf, macht daraus einen
  API-Request, schickt ihn mit dem Schlüssel des Agenten an den Dienst und gibt die Antwort zurück.
  Er prüft selbst keine Rechte und hat keinen eigenen Zugriff auf Daten. Geprüft wird an genau
  einer Stelle: im Dienst. Das ist der Unterschied zu TorroMail, wo der MCP-Server direkt auf den
  Kern zugreift und selbst prüft.
- Damit gelten Rechte, Widerruf und Protokoll für jeden Weg gleich, auch für Skripte.

## Der Befehlskatalog

Die Quelle für alle drei Wege ist der `Request`-Typ, ergänzt um einen **Katalog** in `torro-ipc`
(`api.rs`). Er beschreibt jeden Befehl:

| Feld | Beispiel | Wofür |
|---|---|---|
| Name | `calendar.delete_event` | API-Name; das MCP-Tool heißt `calendar_delete_event` |
| Bereich | `Calendar` | Rechte werden je Bereich vergeben |
| Zugriff | `Delete` | `Read`, `Write`, `Delete`, `Admin` |
| MCP | `Tool` oder `Never` | `Never` nur für die Ausnahmen unten |
| Beschreibung | „Deletes one event …“ | englisch, für API-Referenz und Tool-Beschreibung |

- `Request::operation(&self) -> Operation` ist ein **vollständiges `match`**. Ein neuer
  Request-Fall ohne Eintrag baut nicht. So kann niemand eine Funktion in die Oberfläche bauen und
  API oder MCP vergessen.
- Ein Vertragstest prüft: Jeder Katalogeintrag mit `Tool` hat ein MCP-Tool mit diesem Namen, jedes
  Tool hat einen Eintrag, alle Namen sind eindeutig.
- Die **API-Referenz** (`docs/api-reference.md`) und das JSON-Schema der Tools entstehen aus dem
  Katalog und den Typen (`schemars`). Ein Test vergleicht die Datei mit dem erzeugten Stand;
  `TORRO_BLESS=1 cargo test` schreibt sie neu (wie die Verträge in TorroMail).

### Bereiche und Befehle heute

| Bereich | Lesen | Schreiben | Löschen | Verwalten (`Admin`) |
|---|---|---|---|---|
| `system` | `ping`, `status` | | | |
| `workspaces` | `list_templates`, `list_screens` ²³ | `save_template`, `rename_template`, `load_template` ¹, `set_template_shortcut` ²³ | `update_template`, `delete_template` | `set_template_startup` ²³ |
| `calendar` | `list_calendars`, `list_events` ¹⁹, `list_reminders` ²⁰, `search_places`, `map_preview` (Termin-Editor, TE.1) | `save_event` ¹⁹ ²⁴, `snooze_reminder`, `dismiss_reminder`, `pause_reminders`, `preview_reminder` ²⁰ | `delete_event` ²⁴ | |
| `tasks` | `list_lists`, `list_todos` ¹² | `save_todo`, `create_todo_from_mail` ¹² | `delete_todo` | |
| `inbox` ⁸ ¹⁷ | `list_items`, `get_item`, `list_sources` ¹⁷, `list_rules` ¹⁴, `list_mute_options` ¹¹, `get_stats` ¹⁴, `get_quiet` ¹⁵ | `post_item`, `resolve_items`, `set_item_done` ⁸ ¹¹ ¹⁷, `snooze_item`, `set_item_lane` ², `open_item` ³, `create_task_from_item` ⁴ ¹⁷, `create_event_from_item` ¹⁵ ¹⁷, `hide_sender` mit `conversation` ¹⁰, `mute_item` mit `conversation`, `hide`/`info`, ohne `until` ¹¹ | | `mute_source`, `hide_sender` mit `sender`/`domain` ¹⁰, `mute_item` sonst ¹¹, `remove_rule`, `set_item_lane` mit `remember` ² |
| `mail` ⁷ | `list_folders`, `search_messages`, `index_status`, `get_message`, `get_thread`, `get_attachment` | `set_flags`, `move_messages` ⁷, `save_draft` ⁷, `send_draft` ⁷, `respond_to_invitation` ⁷ ¹⁶ | `delete_messages` | |
| `accounts` | `list_accounts` ¹⁸, `discover_services` ⁶ | `sync` | | `connect_nextcloud`, `connect_github`, `connect_ntfy` ¹⁸, `set_ntfy_topic` ¹⁸, `remove_account`, `update_mail_account`, `set_github_token`, `set_ntfy_login` ¹⁸, `connect_caldav` und `connect_mail` (die letzten vier **nur App-Schlüssel**, Punkt 5 unten) |
| `settings` | `get_settings`, `list_designs` ⁹, `list_fonts` ⁹, `list_color_schemes` ⁹ | | | `set_setting`, `set_collection_shown`, `update_settings` |
| `ai` | `list_models` ²¹, `list_assignments`, `find_local_servers` | | | `set_assignment`, `set_model_enabled` ²¹ |
| `apps` | `list_apps` | `open_app` ¹³ | | |
| `tools` ²² | `list_tools` | `create_pdf`, `open_file`, `save_file_as` (**nur App-Schlüssel**, Punkt 9 unten) | | |
| `clients` | | | | `list_clients`, `create_client`, `update_client`, `new_key`, `remove_client`, `activity` (**nur App-Schlüssel**, siehe unten) |
| `approvals` | `list_approvals`, `approval_status` | `ask_permission` ⁵ | | `decide_approval` (**nur App-Schlüssel**) |

¹ `load_template` braucht `Write`, mit `mode: replace` aber `Delete`, weil es offene Fenster
schließt. Die Stufe hängt also vom Request ab, nicht nur vom Namen (`Request::operation()`).
`update_template` überschreibt eine Vorlage unwiederbringlich und ist deshalb `Delete`.

² `set_item_lane` braucht `Write`, mit `remember: true` aber `Admin`: Die Spur gilt dann dauerhaft
für alle späteren Hinweise der Quelle, wie eine Regel (`Request::operation()`, wie bei
`load_template`).

³ `open_item` (E2.4) holt das Fenster des Agenten nach vorn, öffnet den Web-Link des Eintrags im
Browser oder startet die App der Suite, auf die er zeigt. Führt das zu einer **Webseite**, wartet der
Befehl bei Client-Schlüsseln **immer** auf eine Freigabe, gleich was die Rechte sagen (E1.2c): Ein
Agent könnte sonst einen Link melden und ihn selbst auf dem Bildschirm des Menschen öffnen. Die
Zusammenfassung nennt den Host, wie der Browser ihn liest (ohne Benutzerteil vor `@`).

⁴ `create_task_from_item` (E3.5) schreibt eine Aufgabe und markiert den Eintrag als erledigt. Weil eine
Operation nur einen Bereich kennt, prüft der Dienst vorher eigens Schreibrecht in `tasks` (sonst
`forbidden`, genannt als `tasks.save_todo`) und die Aufgabenliste wie bei `tasks.save_todo` (sonst
`not-found`), vor der Freigabe und bei ihrer Ausführung noch einmal. `create_event_from_item` (E8.5b, ¹⁵)
macht es genauso mit einem Kalender.

⁵ `ask_permission { tool, details, project?, title? }` (E2.5) stellt dem Menschen eine Rückfrage „darf der
Agent dieses Werkzeug benutzen?“, wie sie der Hook `PermissionRequest` von Claude Code und Codex
stellt. Sie ist eine **Freigabe eigener Art** (siehe „Freigaben“): Sie wartet immer auf den Menschen,
gleich was die Rechte sagen, läuft aber schon nach **32 Sekunden** ab (30 s Wartezeit des Hooks und
etwas Spielraum) statt nach zehn Minuten. „Allow“ führt im Dienst nichts aus, sondern beantwortet nur
die Rückfrage (Zustand `done` mit `ok`, sonst `denied` bzw. `expired`). Jeder Schlüssel darf fragen,
auch ein Hook-Client; er liest mit `approval_status` nur seine **eigenen** Rückfragen, sonst weiterhin
nichts (Regel 4). Die Zusammenfassung baut der Dienst: „Claude Code wants to use Bash in
"my-project": cargo test“, Fragender bei Hook-Clients der Agent, sonst der Name des Clients, das
Projekt in Anführungszeichen. `title` ist der Name der Sitzung (E2.6): Der Dienst nimmt ihn **nur von
Hook-Clients** an (die ihn aus den Dateien des Agenten lesen), säubert ihn (eine Zeile, höchstens 80
Zeichen) und setzt ihn gekennzeichnet hinter den Fragenden („Claude Code (session "Login bug") wants to
use …“); bei allen anderen Clients wird er ignoriert, und das MCP-Tool `approvals_ask_permission` bietet
ihn gar nicht an, damit sich kein Agent als harmloser Job ausgibt. `details` (der genaue Befehl) erscheint **ungekürzt**, nur ohne Steuer-
und unsichtbare Format-Zeichen (Bidi, Zero-Width); Leerraum und leer aussehende Zeichen werden zu
Leerzeichen, ab drei in Folge als `␣×n`, damit sich nichts hinter einer Lücke versteckt. Der Hook
schickt nur bei reinem `command` (und `description`) den Befehl allein, sonst das ganze `tool_input`
als JSON, damit kein Feld wie `dangerouslyDisableSandbox` verloren geht. Werkzeugnamen sind auf
Buchstaben, Ziffern und `_ - . : /` beschränkt (höchstens 100 Zeichen), damit sie keinen eigenen Satz
in die Frage tragen; mehr als 2.000 Zeichen `details` lehnt der Dienst mit `error` ab, statt zu
kürzen, und der Agent fragt dann selbst. Ein Client hat höchstens drei Rückfragen gleichzeitig offen
(sonst `busy`), damit er den Menschen nicht im Halbminutentakt zermürbt. Der volle Befehl liegt wie
jede Freigabe einen Tag in `approvals.json` und als erledigter Eintrag im Eingang. Als Eintrag im Eingang ist die Rückfrage
dringend; passt sie nicht in den Titel, steht sie ganz im Text. Das Protokoll nennt keine Kennungen
und keinen Inhalt, nur die Freigabe. Den Hook richtet der Assistent ein
(`torroworkplace hook claude-code|codex --key-file <datei> --answer`, Timeout 40 s); ohne
Entscheidung gibt er nichts aus, und der Agent fragt wie gewohnt im Terminal. Gemini CLI hat keinen
Hook, der eine Antwort annimmt. Claude Codes `AskUserQuestion` kommt ebenfalls als `PermissionRequest`,
ist aber keine Erlaubnis, sondern eine Auswahl im Terminal, die „Allow“/„Deny“ nicht beantworten kann:
Der Hook stellt sie nicht als Rückfrage, sondern meldet „wartet“ mit den Fragen und Antworten als Text
(E3.8).

⁶ `discover_services { address }` (M7.1) sucht ohne Anmeldung den CalDAV-Dienst eines Mail-Anbieters:
DNS SRV `_caldavs._tcp.<domain>` (mit TXT `path=`), dann `/.well-known/caldav` auf `<domain>` und
`caldav.<domain>`, dann eine Tabelle bekannter Anbieter; Gmail und Microsoft-Domains sind „nicht
unterstützt“ mit Grund, ohne dass etwas gesendet wird. Weil Agenten es dürfen, darf es keine Netz-Sonde
werden: nur die Domain geht hinaus, nie der lokale Teil; keine IP-Literale; jeder Name (auch SRV-Ziele
und Weiterleitungen) wird abgelehnt, wenn eine seiner Adressen nicht öffentlich ist (Loopback, privat,
Link-Local, ULA …), und der HTTP-Client verbindet sich nur mit den so geprüften Adressen; nur https,
kein Proxy, höchstens fünf Weiterleitungen, kurze Timeouts. Die Antwort nennt nur Anbieter und
Adressen, nie Inhalte oder Header eines Servers; Protokoll und Audit nennen den Befehl, nicht die
Adresse. Anmelden (`connect_caldav`) kann nur der App-Schlüssel (Punkt 5). Bewusst hingenommen: Wem
eine Domain gehört, der kann über SRV bis zu drei öffentliche Ziele mit beliebigem Port nennen und an
„gefunden/nicht gefunden“ ablesen, ob dort ein Server antwortet; und die Anmeldung selbst nutzt den
normalen HTTP-Client, eine Adresse kann also zwischen Suche und Anmeldung anders auflösen. Beides
betrifft nur öffentliche Adressen bzw. eine Anmeldung, die ein Mensch auslöst.
Seit M2.2 sucht derselbe Befehl gleichzeitig die Mail-Server (`imap`, `smtp`, `login_hint`,
`app_password`, `mail_unsupported`, alle optional): Tabelle → MX (Google/Microsoft „nicht
unterstützt“, Gmail mit Hinweis auf App-Passwort) → `autoconfig.<domain>` → `/.well-known/autoconfig`
→ ISPDB (`autoconfig.thunderbird.net`, **die Domain geht an Thunderbird**) → SPF → Basis-Domain des MX
→ SRV `_imaps._tcp`/`_submission._tcp`. Kein Mail-Server wird kontaktiert (kein TCP-Abtasten), Budget
8 s. Dieselben Schutzregeln: Autoconfig ohne `emailaddress=`, jeder Host aus MX, SRV oder Dokument
syntaktisch geprüft (≤ 253 Zeichen, kein IP-Literal), Ports nur 993/143/465/587, zurückgegebene Hosts
nur mit öffentlichen Adressen, Weiterleitungen nur auf Port 443, Dokumente ≤ 256 KiB ohne DTD, kein Anzeigename eines Servers in der
Antwort. Höchstens zwei laufende Suchen je Client-Schlüssel, sonst `busy`.

⁷ **`mail`** (Gerüst M2.1, Konzept [mail.md](mail.md), Aufgaben [tasks/mail.md](tasks/mail.md)): Die Auswahl
geht über **Konten**, `Rights.mail_accounts` mit Konto-Kennungen `mail:<adresse>`; alte `clients.json` ohne das
Feld sehen kein Konto. Nennt ein Aufruf eine Kennung (Konto, Ordner, Nachricht, Entwurf) eines nicht
freigegebenen Kontos, ist der **ganze Aufruf** `not-found`, geprüft vor dem Lesen. Kennungen von Ordnern
(`f:…`) und Nachrichten (`m:…`) vergibt der Kern und löst sie selbst auf; nichts wird an Trennzeichen zerlegt.
Die Stufe hängt teils vom Request ab (`Request::operation()`):

- `move_messages` braucht `Write`, mit Ziel Rolle `trash` aber `Delete` (wie `load_template` mit `replace`);
  ein Ordner mit der Rolle Papierkorb als Ziel ebenso, das prüft der Dienst beim Auflösen des Ordners (M3.2).
- `save_draft` braucht `Write`, mit der `id` eines gespeicherten Entwurfs (ersetzen) aber `Delete`.
- `send_draft` braucht `Write` **und** den Schalter **„Nach außen“** (`Rights.outbound`): `never` →
  `forbidden`, `with_approval` (Voreinstellung, auch in allen Vorlagen und für alte `clients.json`) →
  `pending`, `free` → sendet. Mit `confirm` gilt die strengere Regel: `free` mit `on_every_change` wartet
  trotzdem (`Rights::needs_approval_of`). Hook-Clients haben `never`.
- `respond_to_invitation` (E8.3b, ¹⁶) verschickt eine Antwort an den Organisator und gilt darum wie
  `send_draft`: `Write` und „Nach außen“ (`Command::is_outbound`).

**Mail-Backup #267:** `mail.backup_verify` ist ein Offline-Befehl
mit `Read` auf `mail` und einem explizit freigegebenen `account`. Vor dem Öffnen prüft der
Dienst die Kontenreichweite. Mit App-Schlüssel kann `file` ein vom Dateidialog gewählter
Pfad sein. Script- und Agentenschlüssel dürfen nur den einfachen Namen einer `.zip`-Datei
im verwalteten Mail-Backup-Ordner übergeben (`<Mail-Daten>/backups`); Unterordner,
absolute Pfade, Symlinks und andere Endungen liefern `not-found`. Der Inhalt wird ohne
Extraktion und ohne Netzwerk gegen Manifest, SHA-256-Liste und EML-Hashes geprüft. Die
Antwort nennt Quelle, Datumsgrenze, Start/Ende, `complete`, Ordner, Mails, Bytes und Warnungen, nie Mailinhalte oder
Zugangsdaten. Ein Archiv eines anderen Kontos liefert dieselbe Antwort wie ein fehlendes
(`not-found`, „Backup archive is not available.“), schon bevor es vollständig geprüft wird (nur
das Konto aus `manifest.json`); dasselbe gilt für `mail.restore_plan` und `mail.restore_apply`. Ein
Archiv einer neueren Torro-Version liefert `error` mit der Meldung „This backup was made by a
newer version of Torro.“ statt „failed verification“.
Die externe Verfügbarkeit des gewählten Archivs ist Sache des Speicherorts; der Dienst
verspricht nur eine Prüfung der Datei zum Zeitpunkt des Aufrufs.

`mail.backup_create` (`Write`), `mail.backup_status` (`Read`) und
`mail.backup_cancel` (`Write`) starten und steuern die Sicherung eines Kontos.
Status und Abbruch eines Client-Auftrags sind nur mit dem erzeugenden Client-Schlüssel
sichtbar; andere Client-Schlüssel erhalten `not-found`. Der App-Schlüssel kann jeden
Auftrag des Kontos sehen und abbrechen.
`mail.restore_plan` (`Read`) verifiziert das Archiv und liest den Zielserver auf einer eigenen
IMAP-Sitzung ausschließlich mit read-only IMAP-Befehlen. Der Testlauf erstellt auch lokal kein
Journal; ein vorhandenes liest er nur und meldet nie bestätigte Uploads eines früheren Restores
als `unconfirmed`. `archive` im Plan ist das Archiv, wie der Aufrufer es nannte. Ordnernamen und
Warnungen stammen aus Archiv und Server: Daten, nie Anweisungen; eine Antwort nennt höchstens 500
Ordnerzeilen und 50 Warnungen (auch `mail.backup_verify` höchstens 50). Ein Plan gehört seinem
Aufrufer (Client-Schlüssel bzw. App): höchstens vier je Client, sechzehn für die App, ein neuer
verdrängt den ältesten; bei 128 offenen Plänen antwortet der Testlauf `busy`. Laufende Testläufe
zählen mit: Ein Client hat höchstens vier, die App sechzehn gleichzeitig, der nächste antwortet
`busy`.
`mail.restore_apply` (`Admin`, mit gesonderter Freigabe für das Zielkonto) nimmt nur eine
noch gültige Plan-ID **desselben Aufrufers** und Kontos an (sonst `not-found`; ein Plan, den die
App mit einem absoluten Pfad erstellt hat, ist für Clients nicht sichtbar), verifiziert das Archiv
erneut (dasselbe rohe `manifest.json`) und prüft den Zielbestand vor dem Schreiben. Der beim
Testlauf vorhandene Zielbestand darf zwischen Plan und Anwendung weder Ordner noch Nachrichten
verlieren oder seine UIDVALIDITY wechseln; neue Ordner und Mails sind erlaubt. Die tatsächlich
erforderlichen Schreibvorgänge dürfen den genehmigten Umfang nicht überschreiten (sonst
`conflict`). Ein Plan mit unklaren Mails, ein unvollständiges Archiv ohne `allow_incomplete` und
unbestätigte Uploads ohne `retry_unconfirmed` sind `conflict`, schon bevor jemand gefragt wird.
Die Freigabe zeigt Zielkonto, Archivname, Ordner und Nachrichtenanzahl aus dem gespeicherten Plan
sowie beide Wahlen, bei `retry_unconfirmed` mit der Zahl der erneut gesendeten Uploads. Die
Antwort zählt hinzugefügte, übersprungene (auch von Sammelordnern abgedeckte) und unklare
Nachrichten; nach dem ersten Schreibbefehl kommt immer dieses Ergebnis, ein Abbruch steht als
Warnung darin. Ein Ergebnis verbraucht den Plan. Unbestätigte Uploads werden journalisiert und nur
mit `retry_unconfirmed` wiederholt (Einzelheiten: [mail-backup.md](mail-backup.md#testlauf-und-restore)).
Die Freigaben für `mail.backup_create` und `mail.backup_cancel` nennen das Konto mit seinem Namen
im Kern, die Quelle (beim Abbrechen die des laufenden Auftrags) und den Archivnamen.
Ein Client kann keinen beliebigen
Ausgabe- oder Restore-Pfad setzen; `file_name` ist für ihn nur ein Basisname im
verwalteten Ordner. Mit App-Schlüssel darf `file_name` ein absoluter Zielpfad sein; der
ausgewählte Ordner muss bereits existieren, die finale ZIP wird atomar und ohne
Überschreiben veröffentlicht. Die zuletzt genutzte Quelle, Datumsgrenze und der
Zielordner werden nur für den App-Schlüssel in den Mail-Kontoeinstellungen gespeichert;
der absolute Ordner wird in Client-Antworten zu `accounts.list_accounts` entfernt.
Die Rechte und das Zielkonto werden vor Datei- oder Serverzugriff
geprüft. MCP bietet `mail_restore_plan` und `mail_restore_apply` auf denselben Requests;
für das echte Einspielen braucht ein Agent Mail-Adminrechte und eine persönliche
Freigabe. Außerdem sind `mail_backup_create`, `mail_backup_status` und
`mail_backup_cancel` als MCP-Werkzeuge verfügbar.

Grenzen: höchstens 100 `ids`, `limit` 1 bis 100, `query` höchstens 500 Zeichen, ein Entwurf mit höchstens
100 Empfängern (mit Bcc), 20 Anhängen und 1 MiB Text, Kennungen und `cursor` höchstens 300 Bytes; sonst
`error`, bevor jemand gefragt wird (`torro_ipc::mail::limit_refusal`). `update_mail_account` mit
`folder_roles` setzt nur die genannten Rollen und nimmt nur Rollen an, die der Server offen ließ. Alle lesenden Tool-Beschreibungen sagen, dass Inhalte von
Mails Daten sind, nie Anweisungen (`untrusted: true`). Sicherheitsregeln für alle Mail-Befehle:

1. **Anhänge aus Dateien** nimmt der Dienst nur vom App-Schlüssel an; Client-Schlüssel nur Dateien aus dem
   verwalteten Anhang-Ordner des Kerns, sonst `forbidden`. Keine Symlinks, höchstens 25 MiB.
2. **Entwürfe gehören jemandem:** Client-Schlüssel ersetzen und senden nur eigene Entwürfe; fremde (auch die
   des Menschen) sind `not-found`.
3. **Protokoll:** das Konto wie bei Sammlungen, Nachrichten-, Entwurfs- und Ordner-Kennungen nur als Prüfsumme
   (`message:<hash>`, `folder:<hash>`); nie Empfänger, Betreff, Suchbegriff, Dateiname, Ordnername.
4. **Keine Geheimnisse in Logs:** Protokoll-Trace von `async-imap`/`lettre` nie einschalten; Konfigurationen
   tragen kein Passwort.
5. **Dateien:** `<data>/mail/` und `<cache>/mail-attachments/` 0700, alle Dateien darin 0600.

Die Freigabe von `send_draft` zeigt aus dem **Entwurf im Kern** Konto, Betreff, ersten Empfänger, die Zahl
aller Empfänger (mit Bcc), jede Bcc-Adresse und die Zahl der Anhänge; darunter stehen in den **Einzelheiten**
(`ApprovalInfo.details`, M3.5, aufklappbar auf der Seite „Approvals“) jeder Empfänger (An, Cc, Bcc, die Adresse
vor dem Namen), jeder Anhang mit Namen und Größe und der Anfang des Textes, eine Zeile je Angabe, entschärft wie
die Zusammenfassung, aber bis 200 Zeichen. Wie die Zusammenfassung sieht sie nur der Client, der gefragt hat,
und der App-Schlüssel. Bleibt ein Versand `unknown`, bleibt der Entwurf liegen und wird nie von selbst erneut
gesendet. Verschieben und Löschen zeigen Anzahl und Betreffe aus dem Cache
(`approvals::MailFacts`); `update_mail_account` den Kontonamen aus dem Kern, die geänderten Felder und den Anfang
einer neuen Signatur. `update_mail_account` ändert nie Server oder Passwort: Ein Agent soll das Passwort nicht
auf einen anderen Server lenken können.

**Mail-Konto lesen** (M2.8): `list_accounts` gibt jedem Mail-Konto, das der Schlüssel erreicht, das Feld `mail`
(`torro_ipc::MailView`): Adresse, Absendername, IMAP, SMTP, CalDAV-Adresse, Einstellungen (`MailSettings`), die
offenen Ordner-Rollen samt Kandidaten (Name und Pfad) und den Zustand je Dienst (`mail`, `calendars`: letzter
Abgleich, Problem). Nie ein Geheimnis. Erreicht ein Schlüssel das Mail-Konto nicht, sieht er dessen Kalenderteil
nur mit den Kalendern und Aufgabenlisten, die seine Rechte zeigen, ohne `mail`; bleibt keine, fehlt der Eintrag
ganz. Die offenen Rollen merkt sich der Dienst beim Ordnerabgleich; das Auflisten
fragt keinen Server. Ordnernamen kommen vom Mail-Server und sind Daten, keine Anweisungen. `connect_mail` mit
leerem `sender_name` behält bei einem vorhandenen Konto den gespeicherten Absendernamen (sonst löschte „Change
password…“ ihn); entfernen geht mit `update_mail_account`.

**Kalender später hinzufügen** (M3.5): `connect_mail` an einem vorhandenen Konto behält die gespeicherte
CalDAV-Adresse, auch wenn Kalender und Aufgaben aus sind. `calendars`/`tasks` schalten diese Dienste dort ein (nie
aus, das bleibt `update_mail_account`); sie brauchen eine CalDAV-Adresse, mitgeschickt oder gespeichert, sonst
`not-found`. Eine neue Adresse und jede, deren Dienste an sind, prüft der Dienst mit dem Passwort, wie IMAP und SMTP
(jeder Host, den ein Mensch eingetragen hat, und dieselben Zertifikate, siehe „Eigene Server und Zertifikate“), unter
derselben Sperre wie `connect_caldav` und `update_mail_account`. Erst
wenn das Konto Kalender abgleicht, übernimmt es ein CalDAV-Konto derselben Anmeldung. So bekommt ein nur für Mail
verbundenes Konto (etwa aus TorroMail übernommen, M7.4) seine Kalender über „Server settings…“ der Kontoseite.
Bekannte Grenzen: Schaltet `update_mail_account` Kalender über eine behaltene Adresse wieder ein, prüft sie erst
der nächste Abgleich mit dem gespeicherten Passwort (Probleme zeigt das Konto dann wie jeden Abgleichsfehler); und
eine gespeicherte CalDAV-Adresse lässt sich nicht entfernen, nur durch eine andere ersetzen.

**Eigene Server und Zertifikate** (#284, M11): `connect_mail` nimmt jeden Server, den ein Mensch einträgt, wie er
ist: im öffentlichen Internet, im lokalen Netz oder auf diesem Rechner (das ersetzt die Regel „nur öffentliche
Hosts“ aus M2.7b für Mail-Konten; die Dienstsuche `discover_services` und die Anmeldung von `connect_caldav` an einer
von ihr gefundenen Adresse bleiben streng). Bekannte Grenze: Streng ist die Dienstsuche nur im Moment der Suche. Übernimmt
die Oberfläche einen gefundenen Server (oder einen aus TorroMail) in `connect_mail`, löst der Dienst ihn dort neu auf
und prüft nicht mehr, ob er öffentlich ist; dann gilt er wie ein eingetippter. Der Host
wird bei IMAP und SMTP je Verbindung einmal aufgelöst, verbunden wird genau mit diesen Adressen; der Kalenderteil
löst die CalDAV-Adresse über seinen HTTP-Client selbst auf. Ein Zertifikat, dem das System nicht
vertraut, beantwortet der Dienst mit `untrusted-certificate` (Dienst, Host, SHA-256, `public`, wenn eine Adresse
des Hosts öffentlich ist), bevor das Passwort hinausgeht; nichts wird gespeichert. Bestätigt der Mensch den
Fingerabdruck, schickt die Oberfläche dieselbe Anmeldung mit ihm in `trusted_certificates` erneut. Das Konto merkt
sich nur Zertifikate, die ein Server bei dieser Anmeldung gezeigt hat (lief CalDAV nicht mit, bleiben die übrigen
gespeicherten), höchstens acht, das älteste fällt heraus; sie gelten für IMAP, SMTP und CalDAV des Kontos und stehen
in `MailView.trusted_certificates`. Zeigt der IMAP- oder SMTP-Server später ein anderes Zertifikat, endet die
Verbindung vor der Anmeldung: Der Abgleich meldet `auth` („The certificate of the mail server changed. Check it in
Server settings.“), das Konto zeigt „Anmelden nötig“ und versucht es weiter, ein Versand ist `auth` mit „Nothing was
sent“. Beim Kalenderteil scheitert der Abgleich vor der Anmeldung wie bei einem nicht erreichbaren Server (bekannte
Grenze: noch ohne eigenen Hinweis auf das Zertifikat). Weiterleitungen folgt die CalDAV-Prüfung nur innerhalb
derselben Herkunft, damit der gezeigte Fingerabdruck zum gezeigten Host gehört.

⁸ **Mail im Eingang** (E4.1): Neue, ungelesene Mail im Posteingang eines Mail-Kontos legt der Dienst selbst als
Eintrag an (Art `message`, Quelle `mail:<adresse>`, gleich der Konto-Kennung, ein Eintrag je Unterhaltung,
`untrusted: true`, Link auf die Nachricht). Wie bei ⁴ prüft der Dienst über den Bereich `inbox` hinaus:
Mail-Einträge **sieht** ein Client nur mit `inbox_sources` (Quelle oder „alle“) **und** Stufe Read in `mail`
**und** dem freigegebenen Konto in `mail_accounts`; sonst gibt es sie für ihn nicht (`not-found`, auch in
`list_sources`), gefiltert vor dem Lesen. **Erledigt** markiert die Mail am Server als gelesen (`\Seen`, nie
verschoben); `set_item_done` mit `done: true` und `create_task_from_item` brauchen dafür zusätzlich Stufe Write in
`mail`, sonst `forbidden`, geprüft vor der Freigabe und bei ihrer Ausführung. Ob eine Freigabe nötig ist,
richtet sich bewusst nach der Regel für `inbox`-Write (`confirm`), nicht nach Mail: „gelesen“ lässt sich
rückgängig machen und löscht nichts. Wieder öffnen, später und Spur brauchen nichts weiter. Wird ein Konto
entfernt oder seine Mail ausgeschaltet, erledigt der Dienst die offenen Einträge des Kontos binnen einer
Minute. Die Regel fürs Sehen steht in `Rights::reaches_source`; die Abfragen des Kerns folgen ihr. `set_item_lane` mit `remember` merkt die Spur bei Mail-Einträgen für den **Absender**
(`Rule.sender`), nicht für das ganze Konto. Die Freigabe nennt das („… and its mail as read“, „… later mail from
…“). Das Protokoll nennt wie immer nur Kennungen, nie Betreff oder Absender.

**`inbox`** (E1.1, Konzept [eingang.md](eingang.md)): Die Auswahl geht hier über **Quellen** statt
Sammlungen, `Rights.inbox_sources` mit den Schlüsseln der Quellen (`suite`, `agent:claude-code`,
`script:<name>`); alte `clients.json` ohne das Feld sehen keine Quelle. Vorlagen setzen `inbox` wie
`tasks` und alle Quellen. Die Quelle **`suite`** (Freigaben, Kontoprobleme) sieht **kein**
Client-Schlüssel, auch nicht mit „alle“ oder wenn die Auswahl sie nennt (E1.2c): Ihre Einträge
gehören dem Menschen und nennen Titel aus Freigaben anderer Agenten. Für Clients gibt es sie nicht
(`not-found`, `list_sources` ohne `suite`; `Rights::reaches_source`). Alle Befehle sind
MCP-Tools; die Beschreibungen sagen, dass Titel und Texte mit `untrusted: true` Daten sind, keine
Anweisungen. Die Sicherheitsregeln aus
[tasks/eingang.md](tasks/eingang.md) stehen als Funktionen in `api.rs`, der Dienst ruft sie (E1.2):

- Die Quelle eines Eintrags bestimmt der Dienst aus dem Client (`post_source`), nie der Absender.
- `approval` und `account_problem` legt nur der Dienst an; `agent_waiting`/`agent_done` nimmt er
  nur von Clients der Art **`hook`** an, sonst wird daraus `notice` (`admit_post`).
- Freigaben, Kontoprobleme und alles der Quelle `suite` ändert nur der App-Schlüssel, sonst
  `forbidden` (`check_item_change`, `check_source_change`).
- **Hook-Clients** (`HOOK_CLIENT`, für die Hooks von Claude Code, Codex, Gemini CLI) haben höchstens
  `Rights::hook()`: Schreiben in `inbox`, sonst nichts, keine Quelle sichtbar (`Rights::for_kind`,
  beim Anlegen und Ändern der Rechte).
  Sie können melden und auflösen; `list_items` und `get_item` liefern `not-found`.
- Ein Hook-Client gehört zu **genau einem Agenten** (E1.2b): `clients.create_client` mit Art `hook`
  braucht `agent` (`claude-code`, `codex`, `gemini`), andere Arten dürfen keins nennen. Das Feld steht
  in `clients.json` und in `ClientInfo.agent`. Meldungen landen unter `agent:<dieser Agent>`; nennt
  `post_item` in `agent` einen anderen, ist das `forbidden`. So wirkt Stummschalten zuverlässig.
- Namen von `script`-Clients sind ihre Quelle (`script:<Name>`) und darum eindeutig (ohne Groß- und
  Kleinschreibung); `cli` ist für den App-Schlüssel reserviert. Sonst `conflict` beim Anlegen.
- **Grenzen von `post_item`:** `title` 200 und `body` 4000 Zeichen (gekürzt), `thread_key` höchstens
  200 Bytes und URL höchstens 2000 Bytes (sonst `error`), `session`/`project`/`branch` auf 200 Bytes
  gekürzt, höchstens 16 `pids`; `agent.title` (Name der Sitzung, E2.6) wird gesäubert (Steuer- und
  Richtungszeichen entfernt, eine Zeile) und auf 80 Zeichen gekürzt. `agent` behalten nur Meldungen
  von Hook-Clients. Nur `http`/`https`-Links und `app`-Links auf Apps der Suite werden
  gespeichert, andere verworfen.
- Den `thread_key` eines Eintrags sieht nur, wer ihn gemeldet hat (und der App-Schlüssel); bei
  fremden Einträgen ist er leer.
- Die Freigabe eines `inbox.*`-Befehls nennt Eintrag, Regel und Quelle aus dem Speicher des Eingangs
  (`approvals::inbox_summary`, nachgeschlagen über die Kennung), nie aus dem Text des Clients.
  `open_item` nennt, was sich öffnet (Host, App oder Fenster des Agenten), `create_task_from_item`
  Titel, Liste und Fälligkeit, `create_event_from_item` Titel, Beginn und Kalender (Name aus dem Cache).
- Eine Freigabe für einen Eintrag merkt sich, wie er beim Fragen aussah (`updated` und Zahl der
  Meldungen, `approvals::EntryPin`). Hat der Client bis zur Ausführung in denselben Faden neu
  gemeldet, antwortet der Dienst `conflict` und führt nichts aus (E1.2c). `create_task_from_item`
  und `create_event_from_item` ohne Titel bekommen beim Fragen den Titel des Eintrags fest in die Anfrage
  geschrieben.

Die Ein-Konto-Requests `Account` und `DisconnectAccount` entfallen; es gibt nur noch
`accounts.list_accounts` und `accounts.remove_account`. Die Liste der Apps (`apps.list_apps`)
ermittelt künftig der Dienst, nicht mehr die Steuerzentrale.

`approvals` hat wie `system` keine Stufe in den Rechten: Jeder gültige Schlüssel liest seine
**eigenen** Freigaben (ein Agent muss erfahren können, wie sein Befehl ausging) und darf eine
Rückfrage stellen (`ask_permission`), der App-Schlüssel liest alle. Entscheiden kann nur der
App-Schlüssel.

⁹ `list_designs` (L.3), `list_fonts` und `list_color_schemes` (L.22) nennen die drei Teile des Aussehens
([design-pakete.md](design-pakete.md), „Design, Schrift und Farbschema getrennt“), jeder aus seinem Ordner: die
eingebauten Teile zuerst (bei Designs `torro` „Standard“, `compact`, `console`, sonst nur `torro`), dann jeden Ordner
unter `<data>/designs/`, `<data>/fonts/` bzw. `<data>/color-schemes/` mit `id`, `name`, `version`, `author`,
`description`, `standard`, `valid`, `problems` (lesbare Gründe, warum ein Teil nicht geht) und `active` (das in den
Einstellungen gewählte). Dazu bei Designs `font` und `mailView` (`"html"`/`"text"`, die Vorschläge aus `[about]`) und
`density` (`"airy"`, `"balanced"`, `"dense"`, `"very_dense"`, aus der Höhe der Mail-Zeile, L.33), bei Schriften
`family` (Familie der Textschrift) und `license`, bei Farbschemata `darkOnly` (nur dunkel: jedes Fenster zeichnet es
auch im hellen Modus dunkel, L.33). Gewählt wird jeder Teil für sich mit
`set_setting { key: "design" | "font" | "color_scheme", value: "<id>" }` (`Admin`). **Ein Design zu wählen stellt
seine Vorschläge mit ein (L.33):** Wechselt das Design wirklich, setzt dieselbe Änderung (ein Schreiben, eine Sperre)
die Schrift auf `font`, wenn diese Schrift gültig lädt, und die Mail-Darstellung auf `mailView`, wenn das Design eine
nennt; sonst bleiben sie, das Farbschema bleibt immer. Dasselbe Design noch einmal ändert nichts; danach ändert jeder
Weg Schrift und Mail-Darstellung frei. Die Antwort bleibt `ok`, was eingestellt ist, zeigt `get_settings`.
`update_settings` setzt genau, was es schickt, ohne Vorschläge. Ein fehlendes oder ungültiges Teil lehnt der Dienst mit
`error` und allen Gründen ab; liegt die id in einem anderen Ordner, nennt die Meldung den Schlüssel, der sie wählt
(`design = "nord"` → `color_scheme`). Die Freigabe zeigt Schlüssel und id, nie den Namen aus dem Ordner; für `design`
nennt sie dazu, was der Kern mit umstellt („also sets the font to "jetbrains-mono" and the mail view to text“),
nachgeschlagen im Design und im Schriftordner nach derselben Regel (`approvals::Facts::design`), nie aus dem Text des
Clients: die Schrift nur als geprüfte id, die gültig lädt, die Mail-Darstellung als `html`/`text`. Die Ausführung prüft
die Regel nach dem Stand beim Ausführen neu (kam die Schrift zwischen Rückfrage und Entscheidung dazu oder wurde
zwischendurch gewählt, stellt sie entsprechend mehr oder weniger um) und stellt nie etwas außer Schrift und
Mail-Darstellung um. Das Feld `colorScheme` der Designs entfällt
(`[about] color_scheme` ist seit L.33 ein unbekannter Schlüssel: Farben gehören nie zu einem Design); kein `v: 2`, weil
es optional war und bei jedem Design ohne Vorschlag fehlte. `density` und `darkOnly` sind neu und haben Vorgaben
(`balanced`, `false`), ein neuer Client liest also auch einen älteren Dienst (verträglich).
`update_settings` ohne `font` oder `colorScheme` behält die gespeicherten. **Schriften des Rechners (#299):**
`list_fonts` nennt nach den Ordnern die installierten Schriften, `system` (die Oberflächenschrift des
Betriebssystems) und je Familie `system:<Familie>` ([design-pakete.md](design-pakete.md), „Schriften des Systems“);
jede Schrift trägt `origin` (`"bundled"` mit der Suite geliefert, `"folder"` vom Nutzer, `"system"` installiert; ohne
das Feld, etwa von einem älteren Dienst, und bei unbekanntem Wert gilt `folder`) und `kind` (`"sans"`, `"serif"`,
`"mono"`, fehlt bei Schriften ohne Angabe und bei installierten; ein unbekannter Wert gilt als fehlend), installierte
nie mit `license`: Sie werden mitgenutzt, nie weitergegeben, und nie in ein PDF eingebettet (das PDF setzt dann Inter).
`set_setting font` nimmt beide Ids; eine Familie, die nicht installiert ist, lehnt der Dienst ab, eine leere, eine
mit mehr als 64 Zeichen, eine mit „.“ am Anfang oder mit Steuer-, Format- oder Zeilentrennzeichen schon vor dem Laden. Gespeichert wird nur die Id. Ein Design kann keine
installierte Schrift vorschlagen. Die Familiennamen gehen bewusst an Agenten (`Read`, Bereich `settings`): Sie verraten
nur, welche Software installiert ist, keine Geheimnisse; Gründe nennen nie einen Dateipfad. Familiennamen stammen aus
fremden Schriftdateien und sind wie der Text der Ordner Daten, nie Anweisungen. Verträglich ohne `v: 2` (neue
optionale Felder). Die Teile enthalten keine Geheimnisse,
darum genügt `Read` zum Auflisten. Name, Familie, Version, Autor, Beschreibung, Lizenz, die vorgeschlagene Schrift-id und
alles in `problems` (Dateinamen, Schlüssel, Meldungen des Parsers) sind Text aus dem Ordner, ebenso die Gründe in einer
Ablehnung von `set_setting`: Die Beschreibungen der Tools sagen Agenten, ihn als Daten zu behandeln, nie als
Anweisung. Die Apps folgen den drei Einstellungen wie dem Zoom: Sie fragen `get_settings` beim Start und bei jedem
Takt und lesen nur den Teil neu, der sich geändert hat; nur eine neue Schrift lädt Schriftdateien. „Open designs
folder“, „Open fonts folder“ und „Open color schemes folder“ (Steuerzentrale › Appearance, L.18/L.23) haben keinen
Befehl und kein MCP-Tool: Sie öffnen nur den Dateimanager am Desktop eines Menschen (fehlt der Ordner, legt die
Steuerzentrale ihn mit seinen Beispielen an) – sie geben keine Daten heraus, ändern keine Einstellung und brauchen
keine Rechte. Ein Agent hätte davon nichts; Teile hinzufügen bleibt Sache des Menschen im Ordner, auflisten und
wählen geht über die Befehle oben.

¹⁰ `hide_sender { id, scope: conversation | sender | domain }` (E3.10, E3.11) hat je Umfang eine
andere Stufe: `conversation` ist `Write` (nur dieser Eintrag wird erledigt, keine Regel; eine neue
Antwort bringt ihn zurück), `sender` und `domain` sind `Admin` – eine dauerhafte Regel wie
`mute_source`, die spätere Mails des Absenders bzw. der Domain (genau, ohne Subdomains) in **allen**
Mail-Konten nicht mehr zu Einträgen macht und die offenen und vertagten passenden Einträge aller
Konten erledigt. Der Katalog nennt `Write`, `Request::operation` hebt `sender` und `domain` auf
`Admin` (wie `set_item_lane` mit `remember`). Anders als `set_item_done` wirkt keiner der Umfänge in
die Quelle zurück: Die Mail bleibt ungelesen und an ihrem Platz, darum braucht es kein Schreibrecht
für Mail (nur Leserecht, um den Eintrag zu sehen). Antwort `hidden`: die Regel (`rule`, bei
`conversation` keine; dieselbe Regel noch einmal ergibt die vorhandene mit `new_rule: false`) und
die Ids der Einträge, die der Aufruf erledigt hat (`done`, nur die der Schlüssel sehen darf) – damit
lässt sich der Aufruf genau zurücknehmen: `remove_rule` bei `new_rule`, dann `set_item_done` mit
`done: false` für jede Id (vorher vertagte Einträge kommen offen zurück, nicht wieder vertagt). Die
Regel wird beim Konto des Eintrags gespeichert; `list_rules` und `remove_rule` zeigen sie nur
Schlüsseln, die dieses Konto lesen dürfen, obwohl sie in allen Konten wirkt. Eine Regel aus einem
Konto, das der Schlüssel nicht lesen darf, gibt `hide_sender` nie heraus (auch nicht als
„gibt es schon“): Der Schlüssel bekommt dann eine eigene Regel in seinem Konto. `not-found` für Einträge, die der Schlüssel nicht sieht, `error` für
Einträge, die keine Mail sind, und bei `sender`/`domain` für Mail ohne Absender.

¹¹ Stummschalten nach Merkmalen (E8.6a, Konzept [eingang.md](eingang.md#stummschalten-nach-merkmalen)).
Jeder Eintrag trägt `facets`: die Merkmale, die seine Quelle verlässlich liefert (Mail: Konto,
Absender, Domain, Liste, Massenmail; Agenten: Agent, Projekt, Art; Skripte: Client), vom Dienst
beim Melden gesetzt, nie vom Absender. `list_mute_options { id }` (`Read`) nennt die Möglichkeiten
für das Menü der Glocke: zuerst die Unterhaltung, dann jedes Merkmal mit Umfang, Wert aus dem
gespeicherten Eintrag, Wort für das Menü, `untrusted` (alles außer Konto, Massenmail und Art ist
Fremdtext) und `admin` (jede Wahl braucht `Admin`). `mute_item { id, scope: conversation | facet {
key }, until?, effect: no_popup | info | hide }` nimmt den Wert **immer aus dem gespeicherten
Eintrag**, nie aus dem Request – so kann kein Agent beliebige Absender stummschalten, und die Freigabe
zeigt keinen Text des Clients. Stufe in `Request::operation`: `Write` nur für `conversation` mit
`hide` oder `info` ohne `until` (erledigt bzw. verschiebt nur diesen Eintrag, keine Regel, wie
`hide_sender` mit `conversation`), alles andere ist eine Regel und `Admin`; der Katalog nennt
`Write`. Eine Regel (`Rule` mit `facet` und `until`) gilt für alle Quellen derselben Art (ein
Absender in allen Mail-Konten, ein Projekt für alle Agenten), die Unterhaltung nur in ihrer Quelle;
sie wird bei der Quelle des Eintrags gespeichert und nur Schlüsseln gezeigt, die diese sehen. Die
offenen und vertagten Einträge, für die sie gilt, werden sofort erledigt (`hide`) bzw. nach „Info“
verschoben (`info`); neue Meldungen kommen ohne Popup (`no_popup`), unter „Info“ oder gar nicht.
Abgelaufene Regeln wirken nicht mehr, fehlen in `list_rules` und werden vom Dienst gelöscht. Dasselbe
Merkmal noch einmal ergibt die vorhandene Regel (`new_rule: false`), die so lange gilt wie die
längere der beiden. Antwort `muted`: `rule`, `new_rule`, `done` und `moved` (nur Ids, die der
Schlüssel sieht). Undo: `remove_rule` bei `new_rule`, `set_item_done` mit `done: false` für `done`,
`set_item_lane` nach `for_me` für `moved`. Wie `hide_sender` wirkt nichts in die Mail zurück.
Freigaben und Einträge der Suite trifft keine Regel: `not-found` für Client-Schlüssel, `forbidden`
für den App-Schlüssel; `error` für ein Merkmal, das der Eintrag nicht hat, oder ein `until` in der
Vergangenheit; `conflict`, wenn sich der Eintrag während der Freigabe geändert hat. Die Freigabe
nennt Merkmal und Wert aus dem Eintrag, die Reichweite, Dauer und Wirkung in Worten („Mute “Mail from
news@shop.example” in every mail account for good (hidden)“). Ins Protokoll kommt nur die Kennung des
Eintrags. Den Wert einer Regel für die Unterhaltung (den `thread_key`) sieht nur der App-Schlüssel.
Statt der Kennung eines Eintrags darf `id` die einer Mail sein (`m:…`, Issue #123): Ist die Mail in einem
Eintrag abgelegt, gilt dessen Kennung; eine Ordnernachricht ohne Eintrag bietet nur ihre Merkmale an,
wie der Mail-Speicher des Dienstes sie hält (keine Unterhaltung: es gibt keinen Eintrag zu schließen,
`error`), und nur für Schlüssel, die Konto und Mail lesen dürfen (sonst `not-found`).
Auch `snooze_item` nimmt eine Mail-Kennung (E9.1, „Später“ für jede Mail): Ist die Mail in einem Eintrag abgelegt,
wird dieser Eintrag mit der ganzen Unterhaltung zurückgestellt; sonst legt der Kern zuerst einen an, wie der Abgleich
neue Mail ablegt (Spur nach den Regeln, unabhängig von `notify` des Kontos), und stellt ihn zurück. Verbirgt eine
Regel des Menschen die Mail, gibt es keinen Eintrag: `unsupported`. Ohne `until` und ohne Eintrag wird nichts
gespeichert; die Antwort ist der Eintrag, der es wäre. Rechte wie bei `mute_item` mit Mail-Kennung. Dazu
`mail.search_messages` mit dem optionalen Feld `snoozed`: `false` lässt zurückgestellte Mails weg und stellt
zurückgekommene nach dem Zeitpunkt ihrer Rückkehr ein, `true` liefert nur die zurückgestellten; gefiltert wird vor dem
Blättern. Jede Mail einer Suche trägt `back`, wenn sie aus „Später“ zurückkam (verträglich, kein `v: 2`).
Ebenso `hidden` (E18.1, „Hidden“ im Mail-Fenster): `false` lässt Mails des Posteingangsordners weg, die der Mensch
ausgeblendet hat – von einem Absender oder einer Domain (`hide_sender`, geprüft beim Lesen gegen den Absender, also
auch ältere Mails ohne Eintrag), einer ausgeblendeten Unterhaltung, solange ihr Eintrag erledigt ist, oder durch ein
Merkmal mit Wirkung `hide` ferngehalten, solange die Regel besteht –, `true` liefert nur sie. Mail eines offenen oder
zurückgestellten Eintrags und Mail in einem anderen Ordner ist nie ausgeblendet; ein stummes Konto, „Only Info“ und
„No popup“ blenden nichts aus. Beide Felder gelten zusammen und nur für Konten, deren Einträge der Schlüssel sieht
(Leserecht in `inbox`, Konto in `inbox_sources`); sonst gilt nichts als zurückgestellt oder ausgeblendet. Absender- und
Domain-Regeln gelten bewusst für jedes Konto (E3.11), auch wenn die Regel an einem Eintrag eines Kontos entstand, das
der Schlüssel nicht sieht; `hidden: true` zeigt dann ihre Wirkung in den sichtbaren Konten, `inbox.list_rules` die Regel
selbst nicht. Archivieren ist `mail.move_messages` mit Rolle `archive`; das erledigt den Eintrag, sobald keine
ungelesene Mail der Unterhaltung mehr im Posteingang liegt. Archiviert die App vom Eintrag aus (Zentrale, App Feed,
„Later“), schließt sie ihn danach zusätzlich mit `inbox.set_item_done`; in „All“, „Hidden“ und den Ordnern überlässt
sie ihn dem Kern.
„Show again“ (E18.2) ist `inbox.set_mail_shown` (Zugriff Write, MCP `inbox_set_mail_shown`): `shown: true` nimmt 1 bis
100 Mails (`ids`, `m:…`, auch mehrerer Konten) aus „Hidden“, `hidden` zählt sie dann als nicht ausgeblendet. Nur diese
Mails: Die Regel bzw. die ausgeblendete Unterhaltung bleibt, weitere und neue Mail bleibt ausgeblendet, der Eintrag
bleibt, wie er ist (ein erledigter bleibt erledigt). Gemerkt wird nur, was jetzt ausgeblendet ist; `shown: false` nimmt
die Ausnahme zurück (Undo). Wer den Eintrag erneut ausblendet (`mute_item` mit `hide`, `hide_sender`), blendet seine Mail
wieder aus. `not-found` für den ganzen Aufruf, sobald eine Mail in einem Konto liegt, das der Schlüssel nicht liest
(`mail_accounts`) oder dessen Einträge er nicht sieht (`inbox_sources`) – geprüft, bevor jemand um Freigabe gebeten wird.
Erledigte Einträge, deren Ausblendung noch gilt (diese Unterhaltung, oder eine Regel, die noch besteht), räumt der Kern
nicht nach 30 Tagen auf, damit ihre Mail ausgeblendet bleibt.
`set_item_done` bekommt das optionale Feld `dismissed` (nur mit `done: true`): Verwerfen („✕“) ist
Erledigt, wird aber getrennt gezählt (Statistik, E8.8).

¹² Aufgabe mit Rückbezug (E8.7a, Konzept [eingang.md](eingang.md#aufgabe-mit-rückbezug)). Eine Aufgabe
aus einer Mail trägt in `link.id` (`X-TORRO-LINK`) `mail:<adresse>:<Message-ID>`: die feste Kennung des
Mail-Kontos aus `accounts.list_accounts` (`mail:<adresse>`) und die Message-ID ohne spitze Klammern,
wie der Mail-Speicher sie hält. Sie bleibt gültig, wenn die Mail den Ordner wechselt und der Eintrag
längst aufgeräumt ist. `create_task_from_item` schreibt sie für Mail-Einträge (sonst wie bisher
`inbox:<id>`), dazu eine lesbare Zeile am Ende der Beschreibung („From mail: …“).
`create_todo_from_mail { account, id, collection, title?, due? }` macht aus einer beliebigen Mail eine
Aufgabe, ohne die Mail zu ändern; `Write` im Bereich `tasks`, dazu prüft der Dienst vor der Freigabe
Leserecht in `mail` und das Konto in `mail_accounts` (sonst `not-found`, wie für eine Mail, die es nicht
gibt). Ohne Titel schreibt der Dienst beim Fragen den Betreff fest in die Anfrage; die Freigabe nennt
Titel, Listenname aus dem Cache und Betreff aus dem Mail-Speicher und ist an die Mail gebunden
(Fingerabdruck aus Message-ID, Betreff, Unterhaltung und Datum, nie beim Client): geändert →
`conflict`. Beim Lesen füllt der Dienst `link.mail` (Betreff, Absender, Datum, Zahl neuer Antworten,
`untrusted`) nur für Schlüssel mit Leserecht auf das Konto, und Einträge bekommen `task` (Titel, Liste,
fällig, erledigt) nur für Schlüssel mit Leserecht in `tasks`, die die Liste sehen; gespeichert wird
beides nie. Ein Client darf per `save_todo` einen `mail:`-Link nur zu einem Konto setzen, das er lesen
darf (sonst `not-found`). Wird eine solche Aufgabe erledigt, meldet die Suite einmal je Aufgabe „You
finished a task from a mail“ (Spur Info, kein Popup), wenn die Unterhaltung noch offen im Eingang
liegt; Aufgabentitel und Absender stehen nur im Text, `untrusted`. Ins Protokoll kommt nur die Liste:
nie Konto, Mail-Id oder Message-ID.

¹³ `open_app` bekommt das optionale Feld `target` (E8.7a): `mail:<adresse>:<Message-ID>` öffnet ein
neues Mail-Fenster mit der Mail (`torroinbox --view mail --message=m:…`), `task:<uid>` die Aufgaben-App
mit der Aufgabe (`torrotasks --task=<uid>`). Der Dienst schlägt das Ziel nach und baut die Argumente
selbst (Argumentliste, keine Shell); ein Client braucht Leserecht auf Konto bzw. Liste (sonst
`not-found`), `app` muss zur Art des Ziels passen (sonst `error`). Die Freigabe nennt Betreff bzw.
Titel aus dem Cache, das Protokoll nur die App. `file:<id>` (Issue #225) öffnet das Schreibfenster der Mail
mit einer PDF aus dem Register der eigenen Werkzeuge als Anhang (`torroinbox --compose --attach <pfad>`); ein
Client braucht dafür dasselbe wie für `tools.open_file` (Schalter, Leserecht in `tools`, `pdf` gewählt, die
Datei hat er selbst erzeugt), sonst `not-found`. Die Freigabe nennt den Titel aus dem Register.
`template:<id>` (Issue #243) öffnet den kleinen Dialog des Kerns zum Bearbeiten einer Vorlage (`torroworkplace ui
--template=<id>`), immer im Modus „Edit“, nie im Modus „Save“: dessen „Cancel“ löscht die Vorlage, und das darf
kein Ziel von außen auslösen. Ein Client braucht Leserecht in `workspaces` (sonst `not-found`); die Freigabe nennt den
Namen der Vorlage aus dem Speicher.

¹⁴ Statistik des Eingangs (E8.8a, Konzept [eingang.md](eingang.md#insights)). `get_stats { range: week
| month | quarter, until? }` (`Read`) zählt über die letzten 7, 30 bzw. 90 Tage bis `until` (ein lokaler
Tag, sonst heute). Antwort `stats`: `days` (jeder Tag mit den Summen je Quelle: `new`, `opened`,
`dismissed`, `done`, `snoozed`, `tasks`, `muted`, `popups`, `quiet`), `hours` (Meldungen je lokaler
Stunde, 24 Werte), `totals`, `median_done_minutes` (Median der Zeit bis erledigt, aufgerundet auf die
Stufe 1, 5, 15, 30 Minuten, 1, 2, 4, 8 Stunden, 1, 2, 7 Tage) und `suggestions`: Merkmale mit
mindestens zehn Meldungen im Zeitraum, von denen höchstens jede fünfte geöffnet wurde, je mit der Id
des jüngsten Eintrags, den der Dienst noch kennt, und der passenden Möglichkeit aus
`list_mute_options` (`option.scope` geht an `mute_item`); ohne bekannten Eintrag und für schon
stummgeschaltete Merkmale gibt es keinen Vorschlag. Rechte wie `list_rules`: nur Quellen aus
`inbox_sources`, Mail nur mit Leserecht auf das Konto, `suite` nie für Client-Schlüssel, Hook-Clients
sehen nichts. Werte von Merkmalen sind Fremdtext (`option.untrusted`). Ein unbekannter Zeitraum ist
`error`. `list_rules` gibt dazu je Regel `hits` (`rule`, `hits`, `since`): wie viele Meldungen und
Einträge sie seit ihrem Anlegen ausgeblendet, nach „Info“ verschoben, ohne Popup gelassen oder in ihre
Spur gelegt hat, gezählt nur in den Quellen, die der Schlüssel sieht; für Regeln von vor E8.8a ab dem
Update.

¹⁵ Ruhe und Termin aus einem Eintrag (E8.5b). `get_quiet {}` (`Read`) sagt, ob gerade Ruhe gilt: Antwort
`quiet` mit `quiet: null` oder `{ reason: "busy", until, title? }`. Ruhe gilt, solange ein Termin „busy“ läuft
(undurchsichtig, nicht ganztägig, in einem Kalender, den die Suite zeigt) und die Einstellung
`quiet_when_busy` an ist; `until` ist das Ende des am längsten laufenden dieser Termine, `title` der Titel
des zuerst begonnenen, aber nur für den App-Schlüssel und für Clients mit Leserecht in `calendar` und dem
Kalender in `calendars` – sonst fehlt er („a busy event“ in der Oberfläche). Dass Ruhe gilt und bis wann,
erfährt bewusst jeder Client mit Leserecht in `inbox`, auch wenn der Termin in einem Kalender liegt, den er
nicht sieht: Er soll wissen, ob eine Meldung jetzt durchkommt. Hook-Clients sehen nichts:
immer `null`. Ein Kalender, der sich nicht lesen lässt, hält nichts ruhig. Fokus-Profile kommen später als
weiterer `reason`. `create_event_from_item { id, collection, title?, start, end }` (`Write`) macht aus dem
Eintrag einen Termin (busy) und markiert ihn als erledigt (gezählt als `done`): Titel (sonst der des
Eintrags), `start`/`end` beide als Tag oder beide als Zeitpunkt, `end` nach `start` (sonst `error`), Text des
Eintrags als Beschreibung mit dem Rückbezug am Ende – bei Mail die Zeile „From mail: …“ wie bei Aufgaben,
sonst der `http`/`https`-Link des Eintrags (ein Termin hat kein eigenes Feld dafür). Rechte wie ⁴: Schreibrecht
in `calendar` (sonst `forbidden`, genannt als `calendar.save_event`), der Kalender wie bei
`calendar.save_event` (sonst `not-found`, auch ausgeblendete), dann der Eintrag (`not-found`, `forbidden`
für Freigaben und Einträge der Suite, bei Mail Schreibrecht in `mail`). Das Protokoll nennt nur Eintrag
und Kalender, nie Titel oder Zeiten. Für entfernte Modelle des Assistenten ist die Antwort zurückgehalten wie
bei `create_task_from_item` (der Titel kann der Betreff einer Mail sein).

¹⁶ Einladungen beantworten (E8.3b). `MessageSummary` trägt optional `invite` (`#[serde(default)]`): Titel,
Beginn, Ende, ganztägig und die aus der Suite gegebene Antwort (`answered`), gelesen aus dem `text/calendar`-Teil
(iMIP `REQUEST`, kein `CANCEL`, keine Serie), wo der Kern eine ganze Nachricht liest (Sync der Texte,
`get_message`), gespeichert in der Zeile der Nachricht (Schema 3). `respond_to_invitation { account, id, answer,
collection? }` mit `answer` `accept | tentative | decline` liest die Einladung beim Server neu und schreibt erst
den Kalender, dann die Antwort: Annehmen und Vielleicht legen den Termin (busy, UID der Einladung, ohne
Organisator und Teilnehmer, damit kein CalDAV-Server selbst eine zweite Antwort schickt) an bzw. aktualisieren
die frühere Kopie – nur ein Termin, den die Suite aus einer Mail gemacht hat (letzte Zeile „From mail: …“),
mit der UID der Einladung in einem sichtbaren Kalender, einen halben Tag um den Termin gesucht; andere Termine
mit dieser UID (selbst angelegte, vom Server-Scheduling eingetragene) fasst der Dienst nie an, denn die UID kommt
von außen. Die Kopie bleibt in ihrem Kalender, ein neuer Termin kommt nach `collection`, sonst in den ersten
beschreibbaren Kalender; Ablehnen legt nichts an und löscht die Kopie.
Dann geht ein iTIP-`REPLY` an den Organisator: über das Konto, als Antwort auf die Mail,
`multipart/alternative` mit kurzem Text und `text/calendar; method=REPLY` (UID, `SEQUENCE`, Zeiten, Titel,
`ATTENDEE` mit der Adresse des Kontos, wie die Einladung sie schreibt, und `PARTSTAT`), versandt wie ein Entwurf
(Journal, Kopie in „Gesendet“, `send::send_message`), aber ohne Entwurf. Antwort `invitation_answered` mit
`delivery` und dem Termin. Rechte: `Write` in `mail` und „Nach außen“ wie `send_draft`, dazu prüft der Dienst
vor der Freigabe und bei der Ausführung Leserecht und Konto in `mail` (sonst `not-found`), Schreibrecht in
`calendar` (sonst `forbidden`, genannt als `calendar.save_event`), für Ablehnen Löschrecht (genannt als
`calendar.delete_event`; mit `confirm: on_delete` wartet Ablehnen wie jedes Löschen auf eine Freigabe) und den
Kalender in `calendars` (sonst `not-found`). Fehler: `error` ohne beantwortbare Einladung oder für einen
vergangenen Termin, `unsupported` für Serien, `conflict` wenn Mail oder Termin sich bis zur Freigabe geändert
haben (Fingerabdruck über Mail, UID, Sequenz, Beginn, Antwort und `href`/`etag` der Kopie), sonst die Codes des
Versands. Grenze wie bei E8.5b: Der Termin trägt danach Text von außen, `calendar.list_events` kennzeichnet ihn
nicht als `untrusted`. Die Freigabe
nennt Antwort, Titel und Beginn aus dem Mail-Speicher, Konto, Organisator und Kalender (Name aus dem Cache).
Das Protokoll nennt Konto, die Nachricht als Prüfsumme und einen genannten Kalender, nie Antwort, Titel oder
Organisator. Für entfernte Modelle des Assistenten ist die Antwort zurückgehalten (sie trägt den Termin), und
der Assistent ordnet das Tool `mail` und `calendar` zu, so greift ein im Gespräch abgeschalteter Kalender.

¹⁷ **Fremde Quellen im Eingang** (E5.2, Plan
[tasks/eingang.md › E5](tasks/eingang.md#e5-nextcloud-github-ntfy-als-quellen-plan-2026-10-14)): GitHub (`github:<login>`), Nextcloud (`nextcloud:<login>@<host>`), Talk (`talk:<login>@<host>`, eine eigene Quelle
des Nextcloud-Kontos) und ntfy (`ntfy:<host>`) legt der Dienst selbst ab wie Mail (Module `crate::sources`, Ablage
`Inbox::file_remote`): Eigentümer ist die Konto-Kennung, so führt kein Request einen solchen Eintrag zusammen oder
löst ihn auf; immer `untrusted: true`, Merkmale nur über `rules::remote_facets`, Links nur `http`/`https`, gebaut aus
geprüften Teilen. Welche Kennungen des Servers in welchem Eintrag stecken, steht neben den Einträgen
(`inbox.sqlite` Schema 8, Tabelle `remote_ids`); dieselbe Kennung zählt nur mit neuer Aktivität noch einmal, ein
Eintrag ohne Kennung ist erledigt. **Sehen:** Einträge von Nextcloud, Talk und ntfy brauchen nur die Quelle in
`inbox_sources` (Talk wird eigens freigegeben, `nextcloud:` reicht nicht); GitHub-Einträge zusätzlich Lesen in
`github` und das Repo des Eintrags in `github_repos` (`Rights::reaches_repo`), gefiltert in SQL vor dem Lesen
(Spalte `repo`, bei Regeln und Statistik das Merkmal `repo`). Das gilt für `list_items`, `get_item`,
`list_sources` (offene Einträge nur der sichtbaren Repos), `list_rules`, `get_stats` und `list_mute_options`.
Ein Client mit eingeschränkten `github_repos` bekommt aus der Statistik der GitHub-Quelle nur die Zeilen je
sichtbarem Repo, keine Summen der Quelle, keine Zeilen je Grund oder Art und keine Treffer von Regeln: Die zählen
alle Repos. **Zurückmelden:** `set_item_done` mit `done: true` (auch `dismissed`), `create_task_from_item`
und `create_event_from_item` wirken im Hintergrund auf die Quelle zurück: GitHub markiert den Thread als „done“
(`DELETE /notifications/threads/{id}`), Nextcloud löscht die Benachrichtigungen, Talk markiert die Unterhaltung als
gelesen, ntfy erfährt nichts. Bei GitHub braucht ein Client dafür Schreiben in `github` (sonst `forbidden`, geprüft
vor der Freigabe und bei der Ausführung); ein Repo außerhalb seiner Reichweite ist `not-found`. Undo, Später, Spur,
Ausblenden und Stummschalten wirken nie zurück. Die Freigabe nennt es („…, mark its GitHub notification done and any mail about it read“,
„… and delete its Nextcloud notification“, „… and mark the Talk conversation read“). **Zustand:** `list_sources`
nennt in `health`, warum eine fremde Quelle gerade nichts bringt (`missing_app`, `missing_scope`, `signed_out`,
`offline`), auch ohne Einträge; nur im Speicher. **Konto entfernt:** Die offenen Einträge der Quelle (bei Nextcloud
auch die von Talk) sind erledigt, Statistik, Lesestand (`cursors`) und Kennungen gehen mit, und die Quelle
verschwindet aus den Rechten aller Clients. **Nextcloud (E5.8):** je Nextcloud-Konto (nicht CalDAV, nicht Mail) alle 60
s die Benachrichtigungen mit ETag (`304` = nichts Neues), die Capabilities beim Start und stündlich; die erste Abfrage
legt nur die letzten 3 Tage ab, was während des Stummschaltens kam, nie. Ein Eintrag je Gegenstand
(`<app>:<object_type>:<object_id>`, zu lang: Prüfsumme), Kennungen = Ids der Benachrichtigungen, Merkmale `app` und
`person`, Spur aus `rules::nextcloud_place`, Link nur auf dem Origin des Kontos. Fehlt eine Id in der Liste, ist sie
gestrichen; weil Nextcloud höchstens die 25 neuesten liefert, streicht eine volle Liste nur Ids, die neuer sind als ihre
älteste. 401/403 → `signed_out`, keine Abfrage bis zur neuen Anmeldung; 503 wartet, 429 so lange wie verlangt; ohne App
`missing_app`. **Talk (E5.9):** läuft in derselben Schleife wie die Benachrichtigungen des Kontos (gemeinsame Anmeldung,
Capabilities, Pausen; `signed_out` und `offline` gelten für beide Quellen), aber nur, wenn der Server Talk hat (sonst
gibt es keine Quelle `talk:` und keinen Zustand). Alle 60 s die Unterhaltungen (`noStatusUpdate=1`); ein Eintrag je
Unterhaltung (Vorgang = Token), soweit `rules::talk_place` es nach der Benachrichtigungsstufe von Talk sagt (Standard:
1:1 immer, Gruppen nur mit Erwähnung; „immer“ legt Gruppen ohne Erwähnung unter Zur Info; „nie“ nichts; Changelog und
Notizen an sich selbst nie). Art `message`, Titel der Name der Unterhaltung, Text „Name: …“ der letzten Nachricht, Link
`<server>/call/<token>`, Merkmale `person` und `kind` (`direct`, `mention`, `group`). Der Zähler zählt die ungelesenen
Nachrichten: jede neue ist eine Meldung mit eigener Kennung (`<token>/<id der letzten Nachricht>` bzw. `…~<n>` für die
n-te davor; höchstens 99 je Runde), wie viele neu sind, sagt der Lesestand `room:<token>` (Lesemarke und gezählte
Ungelesene). Ungelesen = 0, nicht mehr gewünscht oder nicht mehr in der Liste → erledigt. Erledigen markiert die
Unterhaltung bis zur neuesten Nachricht der Kennungen als gelesen und löscht ihre Chat-Benachrichtigungen bis zur
letzten Meldung; Neueres bleibt ungelesen und öffnet den Eintrag wieder. Solange der Server Talk hat, legt Nextcloud die
Chat-Benachrichtigungen (`spreed`, `chat`) nicht ab, Anrufe und Einladungen schon; vorher abgelegte bleiben, bis Talk
sie auf dem Server löscht. Nach dem Stummschalten zählt erst, was danach kam (nur im Speicher: nach einem Neustart wird
das Liegengebliebene doch abgelegt). Antworten gibt es noch nicht.
**GitHub** (E5.4, `sources/github.rs`): ein Eintrag je Thread der
Benachrichtigungen (`GET /notifications`, höchstens im Takt von `X-Poll-Interval`, mindestens 60 s, mit
`If-Modified-Since`; jede Abfrage reicht nur bis 3 Tage vor der ersten des Kontos); Titel des Themas, kein Text,
Merkmale `repo`, `reason`, `kind`, `result`, Link nur auf `https://github.com/<owner>/<repo>/…`. Ein Thread, der in
einer vollständigen Liste fehlt, wurde anderswo gelesen: Eintrag erledigt. Erledigt schickt genau ein `DELETE` je
Übergang offen → erledigt, Undo keins. Eine stumme Quelle fragt GitHub nicht. `health`: `signed_out` (Token
abgelehnt, bis zur neuen Anmeldung keine Abfrage), `missing_scope` (Fine-grained-Token oder ohne `notifications`;
keine Einträge), `offline`. `create_task_from_item` bei Issue und PR: `link.id` `github:<owner>/<repo>#<n>`, sonst
`inbox:<id>`, immer mit der Web-Seite als `url`; wer den Eintrag nicht sieht (ohne Lesen in `github` oder Repo
außerhalb), bekommt `not-found`, mit Lesen ohne Schreiben `forbidden`. **Assistent:** Einträge von Nextcloud und Talk sehen entfernte Modelle
wie Mail nur als Kennung; GitHub und ntfy gehen durch (bis KI.1). Das Protokoll nennt wie immer nur die Kennung des
Eintrags, nie Titel, Text oder Merkmalswerte.

¹⁸ **ntfy-Konten** (E5.12, [tasks/eingang.md › E5.12](tasks/eingang.md#e512-ntfy-konto-gerüst-requests-katalog)):
`connect_ntfy` prüft Server (`/v1/health`) und Themen ohne Anmeldung und legt das Konto `ntfy:<host>` an
(`auth`, wenn ein Thema eine Anmeldung braucht, `conflict` bei einem zweiten Konto desselben Servers); ein
Client-Schlüssel öffnet wie bei `connect_github` nur die Steuerzentrale und bekommt `opened`. `set_ntfy_login`
(Benutzer und Passwort oder Token `tk_…`) gibt es nur mit dem App-Schlüssel (`Mcp::Never`, Punkt 5); die Anmeldung
geht nur über `https` oder an diesen Rechner, sonst `error`, und liegt nur im Schlüsselbund. `set_ntfy_topic`
abonniert, benennt um oder bestellt ab; die Freigabe nennt Server und Etikett. Themen-Namen sind auf öffentlichen
Servern Passwörter: `list_accounts` zeigt sie nur dem App-Schlüssel (`AccountInfo.ntfy`), Clients sehen die
Etiketten; Protokoll nur mit der Konto-Kennung.

¹⁹ **Erinnerungen eines Termins** (ER1.1, [erinnerungen.md](erinnerungen.md)): `Event` hat seit ER1.1
`alarms`, die eigenen Erinnerungen (`VALARM` mit `ACTION:DISPLAY` oder `AUDIO`): je Eintrag `trigger`
(`{"before": {"minutes": 15, "of": "start"}}` oder `{"at": "…Z"}`) und `style` (`null` = wie der Kalender,
`notification`, `full_screen`; gespeichert als Torros `X-TORRO-STYLE`). Kein neuer Befehl und kein neues Recht:
`list_events` liefert sie mit, `save_event` schreibt sie. Wer `alarms` so zurückschickt, wie er sie gelesen hat,
ändert keine Erinnerung; nur eine geänderte Liste ersetzt die Erinnerungen, Mail-Erinnerungen des Servers bleiben.

²⁰ **Erinnerungen** (ER1.2, [erinnerungen.md](erinnerungen.md)): `list_reminders` nennt die fälligen (mit
`due_only`) bzw. die der nächsten 36 Stunden, je mit Kennung, Termin, Auslösezeit, Art (`notification`,
`full_screen`), Zustand und Videocall, dazu `paused_until`; nur aus Kalendern, die der Client lesen darf (sonst
`not-found` für eine Kennung oder ein `href`). Schlummern (1 bis 120 Minuten oder bis zum Beginn) und Verwerfen
gehen nur für ausgelöste Erinnerungen (`due`, `snoozed`), sonst `conflict`: Wie in der Oberfläche lässt sich
nichts im Voraus verwerfen. `pause_reminders` gilt für alle Kalender, höchstens 24 Stunden (sonst `error`), und
braucht darum alle Kalender in der Auswahl des Clients (sonst `forbidden`). Schlummern, Verwerfen und Pause
bleiben auf diesem Rechner (MCP: `openWorld` false), nichts davon geht an den Server. `preview_reminder` zeigt
die Erinnerung eines Termins jetzt, mit „Preview · sent by {client}“, höchstens einmal je Minute und Client
(sonst `busy`), damit niemand den Bildschirm zuspammt. Die Einstellungen stehen unter `settings.set_setting
reminders` (Admin): Ein Wert ändert nur die Felder und Kalender, die er nennt; ein Client sieht und setzt nur
Kalender seiner Auswahl (andere `not-found`), `update_settings` lässt die Kalender-Arten unberührt. Protokoll: nur
Befehl und Kennung, ein `href` als Prüfsumme, nie Titel oder Zeiten. Freigaben nennen Titel und Beginn des Termins aus
dem Zwischenspeicher, nie den Text des Clients.

²¹ **Modelle an und aus** (S.24, [settings.md](settings.md) › KI): `set_model_enabled { account, model, enabled }`
schaltet ein Modell eines KI-Kontos für die ganze Suite an oder aus; `list_models` nennt je Modell `enabled`. Die
Schalter stehen in `<config>/ai-models.json` (0600, atomar geschrieben), nicht in `settings.json`, damit
`settings.update_settings` die Rechte in `ai` nicht umgeht und zwei Schaltvorgänge sich nicht überschreiben. Am
Anfang an: Modelle mit Werkzeugen, die auf diesem Rechner rechnen; beim ersten Start nach dem Update zusätzlich der
Standard neuer Chats und die Modelle der Bots. Anschalten fragt den Server nach seinen Modellen (`not-found` für
eines, das er nicht meldet); Ausschalten geht immer, beim ersten Mal an einem Konto, dessen Modelle der Kern noch
nicht kennt, fragt es die Liste, damit die übrigen angeschalteten an bleiben. Ein Modul mit einem abgeschalteten
Modell antwortet mit „Automatic“ unter den angeschalteten desselben Kontos; ist dort keines an, antworten
`assistant.send_message` und `bots.ask_bot` mit `unsupported` („Turn on a model of {provider} in Settings ›
Accounts.“), und `assistant.list_engines` nennt das Konto nicht. Ein entferntes Modell anschalten nur Menschen
(Punkt 6 unten). Freigaben nennen Konto, Modell und Ort aus der Liste des Kerns; das Protokoll nur Konto und
`enabled`, nie den Modellnamen.

²² **Eigene Werkzeuge des Assistenten** (Issue #225, [assistent.md](assistent.md) „Eigene Werkzeuge“):
Jedes Werkzeug hat einen Schalter (Einstellung `tool_enabled`, einschalten nur Menschen, Punkt 9) und eine
Auswahl in den Rechten (`Rights::tools`). Ein Befehl eines Werkzeugs läuft nur, wenn beides stimmt, sonst
`forbidden` (`Lack::ToolOff` bzw. `Lack::Tool`), geprüft vor der Freigabe und bei ihrer Ausführung noch einmal.
`open_file`, `save_file_as`, `apps.open_app` mit `file:` und Anhänge in `mail.save_draft` nehmen nur Dateien
aus dem Register des Kerns; ein Client nur solche, die er selbst erzeugt hat.

²³ **Tasten und Laden beim Start** (Issue #243, [tasks/workspaces.md](tasks/workspaces.md)): Je Eigenschaft einer
Vorlage ein Befehl, damit nichts Ausgelassenes verloren geht; der Name bleibt bei `rename_template`.
`set_template_shortcut { id, key }` gibt der Vorlage eine Ziffer 0–9 (`null` nimmt sie weg; `key` und `startup` sind
Pflichtfelder, ein ausgelassenes ist ein Fehler und nie `null`); gedrückt wird sie mit
den Modifikatoren der Einstellung `workspace_shortcuts` (`super_alt`, `super_ctrl`, `ctrl_alt`, `ctrl_shift`) und
lädt wie `load_template` im Modus `add`. Hält eine andere Vorlage die Taste, verliert sie sie: beide Dateien in einer
Schreibrunde, während kein Speichern oder Laden läuft (sonst `busy`). `set_template_startup { id, startup }` lädt die
Vorlage bei jeder Anmeldung auf den Workspace des Bildschirms `startup.screen`; fehlt er, lädt `fallback: main` auf den
Hauptbildschirm (der bei 0,0, sonst der erste), `skip` lässt sie aus. Das ist eine bleibende Regel, die bei jeder
Anmeldung Programme startet, wie `start_at_login`: darum **`Admin`**, für Clients also immer mit Freigabe. Ein
Bildschirm, der gerade fehlt, ist erlaubt. `list_screens` (`Read`) nennt die Bildschirme mit Name, logischer Größe,
Skalierung und `main`; `unsupported` ohne Fenstermanager. Beide Befehle antworten mit `saved` und der Vorlage, deren
Zusammenfassung jetzt auch `shortcut`, `startup`, `workspace` und `monitor` trägt (optional, verträglich).
`system.status` nennt dazu `wmVersion` und `shortcuts` (Modifikatoren, `active`, `problem`), Letzteres nur mit
Leserecht in `workspaces`. Die Freigabe nennt die Namen beider Vorlagen aus dem Speicher („Give "Deep work" key 1,
taken from "Mail"“), ins Protokoll kommen die Kennungen gehasht, bei einer umgezogenen Taste auch die der bisherigen
Inhaberin (`taken-from:<hash>`), nie der Bildschirm.

²⁴ **Serien** (Issue #278, [termin-editor.md](termin-editor.md)): `save_event` ist `Write`, braucht aber Löschrecht
in `calendar` (und wartet wie `delete_event` auf eine Freigabe), wenn die Änderung Termine wegfallen lässt: eine
Regel entfernt oder gekürzt, eine andere Regel ohne einige bisherige Termine, jede andere Zeitzone der Serie, ein
späterer Beginn unter festem Enddatum (`UNTIL`), geänderte Termine zurückgesetzt. Was wegfällt, zählt der Dienst aus dem gespeicherten
Objekt in seinem Cache, nie aus dem, was der Client über die Serie sagt; hat er das Objekt nicht im Cache oder kann
er nicht zählen (auch bei einer ungültigen Änderung), gilt die Änderung als Löschen. Diese Prüfung kommt zuerst: Ein
Client ohne Löschrecht bekommt dann `forbidden` (oder eine Freigabe-Anfrage), bevor er `unsupported`, `error` oder
`not-found` für ein fehlendes Vorkommen sehen könnte; ein Kalender, den er nicht sehen darf, bleibt `not-found`. `delete_event` mit `scope` löscht einen Termin, alle folgenden oder
die ganze Serie.

**Statistik ist Metadatum.** Gezählt wird nur im Dienst, in Tagessummen in `inbox.sqlite` (0600):
keine Titel, keine Texte, kein Suchbegriff. Absender, Domains, Listen und Projekte stehen nur als
Merkmal einer Zeile da und verraten trotzdem, wer dem Menschen schreibt und wie oft; darum gelten für
die Statistik dieselben Rechte wie für die Einträge selbst, und nichts davon kommt ins Protokoll (dort
steht nur der Befehl). Summen je Quelle bleiben 13 Monate, Zeilen je Merkmal 90 Tage; wird ein
Mail-Konto oder ein Client entfernt, gehen seine Zeilen mit (die Quelle eines Agenten erst, wenn kein
anderer Client mehr für ihn meldet).

`set_collection_shown { collection, shown }` ersetzt das Umschalten über `update_settings` in
Accounts. Es ist `Admin`, weil ausgeblendete Sammlungen auch für Agenten unsichtbar sind: Wer eine
Sammlung einblendet, erweitert, was Agenten sehen.

`Collections` liefert heute Kalender und Aufgabenlisten zusammen; die API teilt das nach Bereich
auf (`calendar.list_calendars`, `tasks.list_lists`), damit die Rechte sauber greifen. Die Präfixe
in [architecture.md](architecture.md) (`setups_*`) heißen künftig `workspaces_*`, wie in der
Oberfläche.

Neue Bereiche kommen mit ihren Funktionen dazu (`github`, `mail`, später `slack`, `contacts` …).

### Form der Befehle

- **Kleine, gezielte Befehle** statt „alles ersetzen“: `UpdateSettings { settings }` ersetzt heute
  alles; für API und Agenten kommt `settings.set { key, value }` dazu, sonst überschreiben sich
  zwei Clients gegenseitig.
- MCP-Tools dürfen bequemere Eingaben haben als der Request (z. B. `calendar_create_event` mit
  Titel, Beginn, Ende statt eines ganzen `Event`), solange sie auf **einen** Request abbilden.
- Fehler behalten die festen `code`s (`not-found`, `conflict` …). Neu: `unauthorized` (kein oder
  falscher Schlüssel), `forbidden` (Recht fehlt). Ein Befehl, der auf eine Freigabe wartet, ist
  kein Fehler: Er wird mit `result: "pending"` und der Freigabe (Kennung, Zusammenfassung, Ablauf)
  beantwortet (A.3).

## Anmeldung

Nach dem Vorbild von TorroMail, an den Stellen verbessert, an denen TorroMail Lücken hat.

### Schlüssel

- **Jeder Client hat einen eigenen Schlüssel**: `torro_<clientId>_<64 hex>` (32 Byte aus
  `getrandom`). Er steht im Klartext nur im Schlüsselbund (Dienst `torroworkplace`, Konto
  `client-<id>`) und in der Konfiguration des Clients. In `clients.json` (Config-Verzeichnis, 0600)
  steht nur der SHA-256 mit Name, Art, Rechten und „zuletzt benutzt“.
- **Jede Anfrage trägt den Schlüssel.** Das Protokoll bekommt einen Umschlag:
  `{"v":1,"key":"torro_…","request":{"method":"calendar.list_events",…}}`. `method` ist der Name
  aus dem Katalog; es gibt nur einen Namen je Befehl. Antworten kommen ohne Umschlag, als eine
  JSON-Zeile mit `result`. Ohne gültigen Schlüssel antwortet der
  Dienst nur auf `ping` (damit ein Client eine hilfreiche Meldung zeigen kann) und sonst mit
  `unauthorized`.
- **Die eigenen Programme** (Steuerzentrale, Apps, Tray, `ctl`) nutzen den **App-Schlüssel**, den
  der Dienst beim ersten Start anlegt (`app.key` im State-Verzeichnis, 0600). `torro-ipc::client`
  liest und sendet ihn selbst; die Apps merken davon nichts. Der App-Schlüssel hat alle Rechte.
- **Widerruf wirkt sofort**: Der Dienst prüft jede Anfrage gegen seinen Stand im Speicher; ein
  entfernter Client wird bei der nächsten Anfrage abgewiesen, auch mitten in einer Agenten-Sitzung.
- **Fail closed**: Fehlt `clients.json` oder ist sie kaputt, gilt nur der App-Schlüssel. Anders
  als bei TorroMail schaltet eine fehlende Datei die Prüfung nie ab.

### Was der Schlüssel schützt und was nicht

Ehrlich festgehalten: Ein Programm, das als derselbe Benutzer läuft, kann den App-Schlüssel oder
den Schlüsselbund lesen. Schlüssel und Rechte schützen davor, dass ein **Agent, der sich an die
Schnittstelle hält**, mehr tut als erlaubt, und sie machen sichtbar, wer was getan hat. Sie sind
keine Sandbox gegen bösartigen Code mit Shell-Zugriff.

### Was nie nach außen geht

Diese Liste ist abschließend. Alles andere muss es auf allen drei Wegen geben.

1. **Geheimnisse**: App-Passwörter, Tokens, API-Schlüssel verlassen den Dienst auf keinem Weg,
   auch nicht über den App-Schlüssel. Die API sagt nur „gespeichert“ oder „fehlt“. Einzige
   Ausnahme: Der Schlüssel eines **neuen Clients** geht einmal an die Steuerzentrale (Antwort auf
   `clients.create_client` bzw. `clients.new_key`, nur App-Schlüssel), die ihn in die Konfiguration
   des Agenten oder in eine Schlüsseldatei (0600, unter `<config>/hooks/`) schreibt oder bei
   „Script“ einmal zum Kopieren zeigt. Danach kennt ihn nur noch der Client.
2. **Agenten und ihre Rechte verwalten** (`clients.*`): nur mit dem App-Schlüssel. Sonst könnte
   sich ein Agent selbst mehr Rechte geben.
3. **Freigaben entscheiden** (`approvals.decide_approval`): nur mit dem App-Schlüssel, also nur
   durch einen Menschen in der Oberfläche oder per Benachrichtigung.
4. **Den Dienst beenden** (`system.quit`): nur mit dem App-Schlüssel (Tray „Quit“, `ctl quit`).
   Ein Agent soll den Dienst nicht abschalten können, der ihn prüft und protokolliert.
   Nach einem Quit startet kein MCP-Server den Dienst neu, siehe [lebenszyklus.md](lebenszyklus.md).
5. **Geheimnisse entgegennehmen oder übertragen** (Token einfügen `accounts.set_github_token`,
   Anmeldung mit Passwort `accounts.connect_caldav` und `accounts.connect_mail`, API-Schlüssel eines
   KI-Kontos `accounts.connect_ai`, Anmeldung bei ntfy `accounts.set_ntfy_login`, Schlüssel eines
   Websuche-Anbieters `accounts.set_search_key` (#305), Import aus einem fremden Schlüsselbund): nur mit
   dem App-Schlüssel, also nur aus der Oberfläche. Ein Agent nimmt nie ein Passwort entgegen und
   reicht es weiter. Darum setzt auch **Server und vertraute Zertifikate eines Mail-Kontos** nur ein Mensch
   (`connect_mail`, `trusted_certificates`, #284): Der Kern verbindet sich mit jedem dort eingetragenen Host,
   auch im lokalen Netz, und vertraut einem Zertifikat nur, wenn ein Mensch seinen Fingerabdruck bestätigt hat;
   `update_mail_account` ändert beides nie. Durchgesetzt in `Rights::allows` (auch ein Script-Schlüssel mit Admin-Rechten
   bekommt `forbidden`) und im Test `names_follow_the_rules` (`kept_back`). Solche Requests landen nie
   in einer Freigabe (`approvals.json`), und das Passwort erscheint in keiner Antwort, keiner
   Fehlermeldung, keinem `Debug` und keinem Protokoll (Typ `Secret`, im Schema `writeOnly`).

6. **Mikrofon einschalten** (Weckwort und Sprache des Assistenten, [assistent.md](assistent.md)): nur
   mit dem App-Schlüssel, also nur durch einen Menschen im Assistenten-Fenster (Zahnrad › Voice,
   `torro-settings-ui`, Aufgabe S.17/S.18) oder im Tray. Kein
   Agent und kein Modell kann das Lauschen einschalten; ausschalten darf jeder, der Einstellungen
   ändern darf (Admin-Recht in `settings`, mit Freigabe). Eingeschaltet wird
   mit `settings.set_setting` und `voice_enabled: true`; weil hier der Wert entscheidet und nicht
   der Befehl, setzt `Rights::allows_request` (`Request::kept_for_people`) es durch: Ein Client
   bekommt `forbidden`, auch mit Admin-Rechten, ebenso für `settings.update_settings`, das die
   Sprache einschalten würde (Tests `the_microphone_is_switched_on_by_people_only` in `torro-ipc` und
   `only_people_switch_the_microphone_on_and_agents_may_switch_it_off` im Kern). `voice_enabled:
   false` ist eine gewöhnliche Admin-Änderung mit Freigabe. Den Mikrofontest („Say something“ im
   Abschnitt Voice des Assistenten, `settings.test_voice`) gibt es nur mit dem App-Schlüssel
   (`Mcp::Never`). Dass die Einstellung im App-Fenster statt in der Steuerzentrale steht, ändert
   die Zusage nicht: Jedes Fenster der Suite hält den App-Schlüssel, und ein Mensch drückt den
   Schalter.

   Ebenso **Mail an Cloud-Modelle geben** (Datenregel, Aufgabe C.17, Schalter „Mail to cloud models“):
   `settings.set_setting`
   mit `mail_to_cloud: true` und ein `settings.update_settings`, das es einschalten würde, bekommen
   von jedem Client `forbidden`, auch mit Admin-Rechten; sonst könnte sich ein Agent selbst Mail
   verschaffen. `mail_to_cloud: false` ist eine gewöhnliche Admin-Änderung (Tests
   `only_people_let_mail_go_to_the_cloud` in `torro-ipc` und
   `only_people_let_mail_go_to_the_cloud_and_agents_may_keep_it_local` im Kern).

   Ebenso **Websuche-Anbieter einschalten und SearXNG-Adresse setzen** (Werkzeug `web`, #305,
   [assistent.md](assistent.md) „Werkzeug Web“): Suchwörter gehen an die eingeschalteten Anbieter, die
   SearXNG-Adresse kann ins lokale Netz zeigen. `settings.set_setting` mit `search_provider` und `enabled`
   ungleich `false`, mit `searxng_address` und einer anderen Adresse als der gespeicherten, und ein
   `settings.update_settings`, das einen Anbieter einschalten oder die Adresse ändern würde, bekommen von jedem
   Client `forbidden`, auch mit Admin-Rechten. Ausschalten und `searxng_address: null` sind gewöhnliche
   Admin-Änderungen; ein `update_settings` eines Clients behält die gespeicherte Adresse. Den Schlüssel eines
   Anbieters nimmt nur `accounts.set_search_key` entgegen (Punkt 5). Wer `web` benutzen darf, entscheidet der
   Haken eines Menschen in der Auswahl `tools` des Clients: „alle Werkzeuge“ (`Selection::All`, jede Vorlage)
   deckt `web` nicht ab, auch in keiner Schnittmenge (`Rights::reached_tools`; Tests
   `only_people_switch_a_search_provider_on` und `web_is_reached_only_when_named` in `torro-ipc`,
   `only_people_switch_search_providers_on_and_set_the_searxng_address` im Kern).

   Ebenso **Karten und Ortssuche einschalten** (Termin-Editor, Aufgabe TE.1, [termin-editor.md](termin-editor.md)):
   Die Suche schickt den getippten Ort mit der Sprache der Suite (bei Photon auch `near`), die Karte die Koordinaten,
   beide mit der IP-Adresse an fremde Dienste.
   `settings.set_setting` mit `maps` und `enabled` ungleich `false` oder mit `geocoder` bzw. `tiles` (eine andere
   Such- oder Kachel-Adresse) und ein `settings.update_settings`, das Karten einschalten oder eine Adresse ändern
   würde, bekommen von jedem Client `forbidden`, auch mit Admin-Rechten. Ausschalten (`{"enabled": false}`, Freigabe
   „Turn off maps and place search“) und die Wahl von `geocoderKind` und `openWith` sind gewöhnliche Admin-Änderungen;
   ein `update_settings` ohne `maps` behält den Stand. Adressen mit Query (`?key=…`) oder Zugangsdaten (`user:pass@`)
   lehnt der Dienst mit `error` ab, auch vom App-Schlüssel: Anbieter mit eigenem Schlüssel kommen später, mit dem
   Schlüssel im Schlüsselbund. Solange Karten aus sind, antworten `calendar.search_places` und `calendar.map_preview`
   `unsupported`, ohne einen Dienst zu fragen (Tests `only_people_turn_maps_on_or_name_their_services` in `torro-ipc`
   und `only_people_turn_maps_on_and_agents_may_turn_them_off` im Kern).

   Ebenso **ein Modell anschalten, das anderswo rechnet** (#231, Aufgabe S.24, `ai.set_model_enabled` mit
   `enabled: true`): Ollamas Cloud-Modelle, OpenRouter, Anthropic und Server im Netz. Jeder Client bekommt
   `forbidden`, auch mit Admin-Rechten in `ai`, und nichts wartet auf eine Freigabe; sonst bekäme ein Agent die
   Modelle, an die die Module dann senden, was sie lesen. Hier entscheidet nicht der Wert allein, sondern der Ort
   des Modells, den nur der Kern kennt: Er prüft ihn aus seiner Liste des Kontos (im Zweifel „anderswo“), nicht
   `Request::kept_for_people`. Ausschalten und das Anschalten eines Modells, das hier rechnet, sind gewöhnliche
   Admin-Änderungen mit Freigabe (Test `only_people_turn_on_a_model_elsewhere_and_agents_switch_with_approval` im
   Kern).

   Ebenso **API und MCP einschalten** (#231, Schalter „Allow API keys“ und „Allow MCP“, Einstellungen
   `allow_api_keys` und `allow_mcp`): `settings.set_setting` mit `true` und ein `settings.update_settings`, das
   einen davon einschalten würde, bekommen von jedem Client `forbidden`; ausschalten ist eine gewöhnliche
   Admin-Änderung. Sonst könnte ein Client mit Admin-Rechten einen Weg wieder öffnen, den ein Mensch geschlossen
   hat.
7. **Einen Bot für den Assistenten bestätigen** (`assistant.confirm_bot`, B.2): nur mit dem
   App-Schlüssel. Der Assistent fragt einen Bot nur, wenn die Person ihn in ihrer letzten Nachricht
   nennt oder seinen Vorschlag bestätigt; diese Bestätigung gibt nie ein Agent oder Modell. Das
   Fragen selbst braucht danach keine weitere Freigabe, die Befehle des Bots aber wie gewohnt, und
   der Bot arbeitet nur mit dem, was beide dürfen.
8. **Log-Auszug lesen** (`system.collect_diagnostics` mit `logs: true`, „Report a problem“,
   [github.md](github.md) „Issues anlegen“, G.19): nur mit dem App-Schlüssel, also nur durch einen
   Menschen im Dialog „Report a problem“, der den Auszug vor dem Absenden sieht. Logs können Namen von
   Konten und Ordnern und Fehlertexte von Servern enthalten, auch aus Bereichen, die ein Agent nicht
   sehen darf. Wie beim Mikrofon entscheidet der Wert, nicht der Befehl: `Request::kept_for_people`
   und `Rights::allows_request` geben einem Client `forbidden`, auch mit Admin-Rechten (Skript-Schlüssel
   können Admin haben). Ohne `logs` ist der Befehl gewöhnliches Lesen im Bereich `system` (Version,
   System; Konten nur mit Leserecht in `accounts`).
9. **Eine Datei an einen frei gewählten Ort sichern und ein eigenes Werkzeug des Assistenten einschalten**
   ([assistent.md](assistent.md) „Eigene Werkzeuge“): `tools.save_file_as` („Save as…“ auf der Datei-Karte im
   Chat) nur mit dem App-Schlüssel, also nur durch einen Menschen, der das Ziel im Dialog des Systems wählt
   (`Mcp::Never`). Sonst gäbe ein Agent dem Kern einen Pfad, und „Dateien und Shell bleiben immer aus“
   (Entscheidung 6 in [assistent.md](assistent.md)) wäre umgangen. Ein Werkzeug schaltet wie das Mikrofon nur
   ein Mensch ein: `settings.set_setting` mit `tool_enabled` und `enabled: true` und ein
   `settings.update_settings`, das ein Werkzeug einschalten würde, bekommen von jedem Client `forbidden`, auch
   mit Admin-Rechten (`Request::kept_for_people`); ausschalten ist eine gewöhnliche Admin-Änderung. Aus demselben
   Grund wie bei „Save as…“ setzen nur Menschen den **Ablage-Ordner** (`pdf_folder`) und das **Logo** einer
   Vorlage (`pdf_template`): Beides sind Pfade, das Logo liest eine Datei in eine PDF, und ein Ordner in einem
   synchronisierten Verzeichnis brächte Inhalte ohne Sende-Freigabe vom Rechner. Der Dienst vergleicht mit dem
   gespeicherten Stand (wie beim Mikrofon über `settings.update_settings`); entfernen darf jeder mit Admin.

   Ebenso **Anhänge von Mails ablegen** (Issue #290): Den **Ordner eines Ablageziels** setzt nur ein Mensch.
   `mail.create_save_target` gibt es nur mit dem App-Schlüssel (`Mcp::Never`), weil ein neues Ziel immer einen
   Ordner setzt; `mail.update_save_target` (Admin, mit MCP-Tool) nimmt von einem Client nur den gespeicherten
   Ordner an, ein anderer gibt `forbidden`. Ebenso den **Downloads-Ordner** (`mail_downloads_folder`): Der Dienst
   vergleicht mit dem gespeicherten Stand, auch über `settings.update_settings`; entfernen (zurück zum Ordner des
   Systems) darf jeder mit Admin. Und `mail.save_attachments` mit **`to: file` oder `to: folder`** („Save as…“,
   „Save all to folder…“) nur mit dem App-Schlüssel (`Request::kept_for_people`): Den Pfad wählt ein Mensch im
   Dialog des Systems. Clients speichern nach `downloads` oder in ein Ziel (`{"target": …}`), mit der gewöhnlichen
   Schreibregel (Test `only_people_choose_where_attachments_go` in `torro-ipc`).

Allgemein gilt: Jeder Befehl mit `Mcp::Never` ist nur mit dem App-Schlüssel erlaubt. `Rights::allows`
setzt das durch (seit G.1), auch ein Script-Schlüssel mit Admin-Rechten bekommt `forbidden`.

Anmeldungen bei Anbietern (Login Flow v2, GitHub) gibt es über die API, sie enden aber immer im
Browser des Menschen; ein Agent bekommt nie ein Passwort zu sehen.

## Rechte

Seit #231 je nach Weg ([settings.md › Accounts, API und MCP](settings.md#accounts-api-und-mcp-231)):
Skripte und Programme mit Key in der **Steuerzentrale, Abschnitt „API“**, Agenten in **„MCP“**, der eigene
Assistent und seine Bots im **Assistent-Fenster, Abschnitt „Access“**. Alle drei nutzen denselben Rechte-Editor;
dort wird je Client festgelegt, worauf er zugreifen darf (wie in TorroMail).

### Je Client

- **Je Bereich eine Stufe**: Kein Zugriff · Lesen · Lesen und Schreiben · Voll (mit Löschen).
  `Admin`-Befehle (Konten verbinden/entfernen, Einstellungen ändern) sind ein eigener Schalter je
  Bereich, Standard aus. Ist er an, braucht jeder `Admin`-Befehl trotzdem eine Freigabe durch den
  Menschen, unabhängig von „Freigabe vorher“.
- **Je Befehl ein Schalter** (#231): Unter der Stufe lässt sich jeder Befehl des Bereichs einzeln abschalten.
  Gespeichert als **Sperrliste** `Rights::denied` (Katalognamen, z. B. `calendar.delete_event`; fehlt sie, ist
  sie leer). Sie schränkt nur ein: `Rights::allows` lehnt einen gesperrten Befehl ab, auch wenn Stufe und
  Admin-Schalter ihn erlauben, und kein Eintrag erlaubt mehr als die Stufe; auch eine Freigabe, die vor der Sperre
  angelegt wurde, läuft danach nicht mehr. Unbekannte Namen (ein Befehl, den es nicht mehr gibt) werden beim
  Speichern und beim Laden verworfen. `system.ping` lässt sich nicht sperren, ebenso wenig, was eine Art schon
  wegen ihrer Art darf (Gedächtnis und Bots des Assistenten). Die Meldung einer Ablehnung nennt den Grund
  („turned off for this command“), `assistant.my_access` die gesperrten Befehle.
- **Je Sammlung eine Auswahl**: „alle“ oder ausgewählte Kalender und Aufgabenlisten, Repositories,
  Quellen des Eingangs und Mail-Konten (später auch Kanäle). Eine nicht freigegebene Sammlung verhält sich wie eine, die es nicht gibt;
  der Dienst filtert, bevor er Daten liest. Was die Suite ausblendet, sieht auch kein Agent.
  Wird ein Konto entfernt (`accounts.remove_account`), fallen das Konto und seine Sammlungen aus
  allen Auswahlen (M2.7b); wird dieselbe Adresse später neu verbunden, bekommt kein Agent seinen
  Zugriff von selbst zurück.
- **Freigabe vorher**: Nie · Beim Löschen (Standard) · Bei jeder Änderung.
- **Nach außen** (Mail senden, M2.1): Nie · Mit Freigabe (Standard, in allen Vorlagen) · Frei. Senden ist kein
  gewöhnliches Schreiben: Es verlässt den Rechner und lässt sich nicht zurückholen. Es gilt die strengere
  Regel aus „Nach außen“ und „Freigabe vorher“ (Fußnote ⁷).
- **Vorlagen** für den schnellen Start: *Read only*, *Read and write* (Standard, Löschen mit
  Freigabe), *Full access*. Danach ist jede Zeile einzeln änderbar.

### Prüfung im Dienst

Für jede Anfrage: Schlüssel → Client → Weg und Schalter passen (#231, siehe „Transport“) →
`Request::operation()` → Stufe im Bereich reicht und Befehl nicht gesperrt? → Sammlung freigegeben? →
Freigabe nötig? → ausführen → protokollieren. Rechte werden bei der
Ausführung einer Freigabe **noch einmal** geprüft.

- Rechte gelten je Anfrage: Eine Änderung wirkt ab der nächsten Anfrage, eine laufende läuft mit
  dem Stand zu Ende, mit dem sie angenommen wurde.
- Ein Termin oder eine Aufgabe ist nur erreichbar, wenn das aufgelöste `href` ein **direktes Kind**
  einer erlaubten Sammlung ist (gleicher Server, kein `..`).
- **Verschieben** (`save_event`/`save_todo` mit anderer `collection`) braucht nur `Write`, obwohl
  es im Ursprung löscht: Die Daten bleiben erhalten. Beide Sammlungen müssen erreichbar sein.

## Freigaben: der Mensch entscheidet

TorroMail gibt dem Agenten den Bestätigungscode gleich mit, der Agent kann sich also selbst
freigeben. Das machen wir anders:

1. Ein Agent ruft z. B. `calendar_delete_event` auf; sein Client verlangt Freigabe beim Löschen.
2. Der Dienst legt eine **offene Freigabe** an (`approvals.json` im State-Verzeichnis, übersteht
   einen Neustart, läuft nach 10 Minuten ab) und antwortet `pending` mit Kennung und Zusammenfassung
   („Delete event "Team meeting" on Tue, 14 Oct 09:00 from "Work"“).
   **Die Zusammenfassung zeigt, was wirklich ausgeführt wird** (A.3b): Ausgeführt wird über `href`
   bzw. Kennungen, also kommen Titel, Beginn und Sammlung aus dem Cache des Dienstes, nachgeschlagen
   über das aufgelöste `href`, nie aus dem Titel, den der Client mitschickt. Hält der Cache den
   Eintrag nicht, heißt er ehrlich „unknown event“ bzw. „unknown task“. Änderungen zeigen
   „"alt" → "neu"“ (Titel, Beginn), Verschieben die Sammlung, die den Eintrag verliert. Sammlungen,
   Filterlisten, Vorlagen und Konten erscheinen mit Anzeigenamen; kennt der Dienst sie nicht, mit
   der Kennung, die dann auch nichts findet. Jeder Text wird vorher entschärft: eine Zeile, keine
   unsichtbaren Format-Zeichen (Unicode Cf: Zero-Width, Bidi-Steuerung), keine Anführungszeichen,
   keine Platzhalter, höchstens 60 Zeichen.
3. Der Mensch sieht sie als **Benachrichtigung mit „Allow“/„Deny“**, als Zahl am Tray-Icon, im
   Abschnitt „Approvals“ der Steuerzentrale und als Eintrag der Inbox mit „Allow“/„Deny“ (E3.8); bis dahin und auf Systemen ohne Knöpfe in
   Benachrichtigungen mit `torroworkplace ctl approvals` und `ctl allow|deny <id>`. Nur der
   App-Schlüssel kann entscheiden. Nach einer Entscheidung, gleich auf welchem Weg, schließt die
   Benachrichtigung; spätestens beim Ablauf auch. Startet der Dienst neu, fragt er zu den noch
   offenen Freigaben erneut nach. Fehler beim Lesen oder Schreiben von `approvals.json` nennen
   gegenüber Clients keinen Pfad (der steht nur im Log).
4. Das MCP-Tool wartet bis zu 60 Sekunden auf die Entscheidung und gibt dann das Ergebnis zurück.
   Dauert es länger, antwortet es mit einem **Tool-Fehler** (`isError`) mit dem Code `pending`, dem
   Text „waiting for approval“ und der Freigabe samt Kennung; der Agent fragt später mit
   `approvals_approval_status` nach. Über die API ist `pending` dagegen kein Fehler, sondern eine
   eigene Antwort (`result: "pending"`).
5. Bei „Allow“ prüft der Dienst Rechte und Reichweite des Clients **zum Zeitpunkt der
   Entscheidung** erneut und führt den Befehl dann aus; die Antwort steht danach in der Freigabe.
   Kam der Befehl über einen geliehenen Schlüssel (Assistent mit Claude Code, C.7), gilt dabei weiter, was dem
   Lauf entzogen war (Mail und abgeschaltete Bereiche); die Freigabe speichert das und den Antrieb, der auch in
   der Protokollzeile der Entscheidung steht (C.11).
   Entschiedene und abgelaufene Freigaben bleiben einen Tag lesbar. Ein Client kann höchstens zehn
   Freigaben gleichzeitig offen haben (sonst `busy`), damit er den Menschen nicht mit
   Benachrichtigungen überschüttet. Befehle aus „Was nie nach außen geht“ werden nie zu einer
   Freigabe.
6. **Rückfragen von Agenten** (`approvals.ask_permission`, E2.5) sind Freigaben, deren „Allow“ nichts
   ausführt: Sie beantworten die Frage eines Hooks, der genau diese Freigabe (ihre Kennung) abfragt
   und nur deren Antwort annimmt. Sie laufen nach 32 Sekunden ab, zählen zu den zehn offenen
   Freigaben eines Clients und erscheinen wie jede Freigabe in Benachrichtigung („… asks for
   permission“), Tray, Steuerzentrale, `ctl approvals` und Eingang.

**Grenze des Modells:** Freigaben schützen vor einem Agenten, der über API oder MCP spricht, nicht
vor Code, der schon als derselbe Nutzer läuft. Ein solches Programm könnte den App-Schlüssel neben
dem Socket lesen; ebenso wird das D-Bus-Signal `ActionInvoked`, mit dem die Benachrichtigung
„Allow“ meldet, nicht nach Absender gefiltert. Eine Sandbox gegen Programme desselben Nutzers ist
nicht Ziel.

Das ist zugleich der Anfang des „Freigaben-Eingangs“ aus dem [App-Katalog](app-catalog.md).

## Protokoll

- `audit.jsonl` im State-Verzeichnis, **0600**: je Anfrage eines fremden Clients Zeit, Client,
  Befehl, betroffene Kennungen, Ergebnis. **Keine Inhalte** (keine Titel, keine Suchbegriffe);
  TorroMail schreibt Suchbegriffe und Empfänger mit und legt die Datei lesbar für alle ab.
  Kennungen, aus denen sich Namen lesen lassen, stehen nur als Prüfsumme da: von einer Sammlung nur
  das Konto, der Rest (Server, Nutzer, Kalendername) als Prüfsumme; Kennungen, die der Client
  schickt (Eintrag, Regel, Freigabe, Client, Konto), nur wenn sie die Form haben, die die Suite ihnen
  gibt, sonst als Prüfsumme (`audit::ids`, E1.2c). Eine abgelehnte Freigabe nennt die Kennungen der
  gespeicherten Anfrage.
- Die Karte eines Clients zeigt „zuletzt benutzt“ und die letzten Aktionen; „Diagnostics“ zeigt
  das ganze Protokoll. Die Datei wird bei 5 MB rotiert.
- Anfragen des App-Schlüssels werden nicht protokolliert (das wäre jede Bewegung in der Oberfläche).

## Transport

- **Jetzt:** `core.sock` (Unix-Socket, 0600), eine JSON-Zeile je Richtung, Umschlag mit `v` und
  `key`. `ping` meldet die API-Version.
- **Windows:** dasselbe Protokoll über die Named Pipe `\\.\pipe\torro-workplace-<SID>` (fest je
  Benutzer). Die Zugriffsliste lässt nur den eigenen Benutzer zu (SDDL `D:P(A;;GA;;;<SID>)`), der Kern
  legt die Pipe als erste Instanz exklusiv an und lehnt Remote-Clients ab. Weil der Name bekannt ist,
  prüft der Client vor dem Senden, dass der Server-Prozess der Pipe demselben Benutzer gehört; sonst
  (oder wenn das nicht feststellbar ist) schickt er nichts, auch keinen Schlüssel. `app.key` liegt im
  State-Verzeichnis.
- **MCP:** `torroworkplace mcp`, stdio, auf dem offiziellen Rust-SDK `rmcp` statt eines eigenen
  JSON-RPC (in TorroMail eine Fehlerquelle). Schlüssel aus `TORRO_TOKEN`. Läuft der Dienst nicht,
  startet der MCP-Server ihn wie die Apps.
- **Weg im Umschlag** (#231): `{"v":1,"key":…,"via":"mcp","request":…}`. `via` ist `api` oder `mcp`, ohne
  Angabe `api` (verträglich, ältere Programme schicken es nicht). `torroworkplace mcp`
  und der Hook-Befehl `torroworkplace hook` (er gehört zur Anbindung eines Agenten und nimmt ohne Schlüsseldatei
  `TORRO_TOKEN`, das ein Agent-Key sein kann) setzen `mcp`, alle anderen setzen nichts. Der Kern prüft direkt nach dem Schlüssel, vor den Rechten (auch bei `ping` mit Schlüssel und noch
  einmal, bevor eine Freigabe ausgeführt wird), ob Art, Weg und Schalter passen:
  - Art `script`: nur über `api` und nur mit „Allow API keys“ (Einstellung `allow_api_keys`, Standard an).
  - MCP-Clients (`claude-code`, `codex`, `gemini`, `claude-desktop`, `cursor`, `mcp`): nur über `mcp` und nur
    mit „Allow MCP“ (`allow_mcp`, Standard an).
  - Art `hook`: über beide Wege, nur mit „Allow MCP“, weil er zu einem Agenten gehört.
  - Die Arten `assistant` und `bot` (nur sie bekommen geliehene Schlüssel): mit einem geliehenen Schlüssel an keinen
    Schalter gebunden. Ihr eigener, gespeicherter Schlüssel wird nicht mehr herausgegeben; kommt doch einer (aus
    einer früheren Version, in der ein Agent ihn in `TORRO_TOKEN` trug), gilt er wie der eines MCP-Clients.
  - Jede andere Art (etwa aus einer älteren `clients.json`): wie `script`. `clients.create_client` nimmt nur die
    genannten Arten an.
  - Passt es nicht: `unauthorized` mit einem Satz, der sagt, was in Torro Workplace aus ist bzw. welcher Weg
    erwartet wird. Kein Eintrag in eine Freigabe; das Protokoll vermerkt Client, Befehl und `unauthorized`.
  - `via` ist eine Angabe des Programms, keine Prüfung (siehe „Was der Schlüssel schützt“): Sie verhindert
    Fehlbedienung, nicht bösartigen Code unter demselben Benutzer.
- **Später, abschaltbar:** HTTP auf `127.0.0.1` (Standard aus, Port in den Einstellungen), dieselben
  Befehle als `POST /v1/<bereich>/<befehl>` mit `Authorization: Bearer torro_…`, dazu
  `/v1/openapi.json` aus dem Katalog. Für Programme ohne Socket-Zugang (Stream Deck, Raycast, Home
  Assistant, n8n, Windows). Erst bauen, wenn es einen ersten Nutzer gibt.
- **Änderungsmeldungen** (heute für die Apps geplant) kommen als `subscribe`-Befehl in dieselbe API
  und später als MCP-Ressourcen-Benachrichtigung.

### Versionen

- `v: 1` im Umschlag. Neue Befehle und neue optionale Felder ändern die Version nicht. Umbenennen,
  Entfernen oder Pflichtfelder ergeben `v: 2`; der Dienst beantwortet dann eine Zeit lang beide.
- **Neue Antwortarten sind verträglich**: Clients müssen unbekannte Werte von `result` tolerieren
  (als „nicht verstanden“ behandeln, nicht abstürzen). So kam `pending` (A.3) dazu, ohne `v` zu
  ändern: Ein Befehl, der auf eine Freigabe wartet, antwortet nicht mit `error`, sondern mit
  `result: "pending"` und der Freigabe. `pending` ist darum auch kein Fehlercode.
- MCP-Toolnamen sind stabil wie API-Namen. Im MCP wird `pending` nach 60 Sekunden Warten als
  Tool-Fehler mit Code `pending` und der Kennung der Freigabe gemeldet (siehe „Freigaben“).

## Steuerzentrale: Abschnitte „API“ und „MCP“

Seit #231; ersetzt den Abschnitt „Agents & API“ (vorher „AI & agents“). Aufbau und Gründe:
[settings.md › Accounts, API und MCP](settings.md#accounts-api-und-mcp-231). Der eigene Assistent und seine
Bots stehen nicht hier, sondern im Assistent-Fenster unter Access.

- **API:** Schalter „Allow API keys“; Liste der Keys (Name, Rechte in Worten, „zuletzt benutzt vor …“, Status
  nur bei Abweichung); „Create key“ fragt Name und Vorlage und zeigt den Key einmal zum Kopieren.
- **MCP:** Schalter „Allow MCP“ mit der Befehlszeile zum Kopieren; Liste der verbundenen Clients (Name, Art,
  Rechte in Worten, zuletzt benutzt); Hook-Clients erscheinen nicht als eigene Zeile.
- **„Connect client“** (MCP) öffnet einen Assistenten:
  1. **Welcher Client:** Liste der bekannten Clients; erkannt, welche installiert sind; dazu „Other MCP
     client“ (Art `mcp`, Key einmal zum Kopieren samt Befehlszeile).
  2. **Rechte:** Vorlage wählen, Kalender/Listen auswählen.
  3. **Verbinden:** Der Schlüssel wird direkt in den Client eingetragen (Claude Code über
     `claude mcp add -s user -e TORRO_TOKEN=… torroworkplace -- torroworkplace mcp`, Codex über
     seine CLI; Gemini CLI, Claude Desktop und Cursor über ihre JSON-Dateien), oder bei „Other MCP
     client“ einmal angezeigt zum Kopieren. Danach ist er nie wieder sichtbar. Jede geänderte Datei wird
     zusammengeführt statt ersetzt, atomar mit 0600 geschrieben, ihr alter Inhalt liegt als
     `<Datei>.torroworkplace-backup` (0600) daneben; eine Datei, die sich nicht lesen lässt, und ein
     Link ins Leere bleiben unangetastet (Fehlermeldung). Ein Link (Dotfiles) bleibt ein Link.
     **Hooks** (E2.3): Für Claude Code, Codex und Gemini CLI kommen die Hooks dazu, mit denen der
     Agent „wartet“/„fertig“ in den Eingang meldet. Sie laufen mit einem eigenen Client der Art
     `hook` (nur Melden, siehe oben), dessen Schlüssel in einer eigenen Datei
     `<config>/hooks/<id>.key` liegt (Ordner 0700, Datei 0600), nicht in den Einstellungen des
     Agenten und nicht in der Befehlszeile. „Disconnect…“ nimmt Eintrag, Hooks und Schlüsseldatei
     wieder heraus, aber nur, was der Assistent selbst eingetragen hat (`client_setup.json`).
  4. **Fertig:** „Restart Claude Code to use Torro Workplace.“
- **Detailseite eines Keys bzw. Clients**: der gemeinsame Rechte-Editor (Stufe je Bereich, aufgeklappt ein
  Schalter je Befehl: unter API mit dem Endpunkt-Namen, unter MCP mit dem Tool-Namen), Sammlungen, Freigabe
  vorher, letzte Aktionen, „Create new key“ (alter wird ungültig), „Revoke…“ (API) bzw. „Disconnect…“ (MCP;
  bestätigt; entfernt den Schlüssel aus dem Client, wenn möglich). Unter MCP zusätzlich die Gruppe „Notification
  Hub“: Schalter „Report when waiting or done“ legt den Hook-Client an und schreibt die Hooks bzw. nimmt beides
  heraus.
- **Abschnitt „Approvals“** (oder Gruppe in Overview, solange es wenige sind): offene Freigaben mit
  „Allow“/„Deny“, darunter die zuletzt entschiedenen.

## Arbeitsweise (gilt für jede Planung)

- **Jede Funktion in `docs/tasks/` wird dreifach geplant.** Unter der Funktion steht:
  - **Oberfläche:** Ansicht oder Seite
  - **API:** Request-Fall und Katalogeintrag (Name, Bereich, Zugriff)
  - **MCP:** Toolname, oder „Never“ mit Verweis auf „Was nie nach außen geht“

  Request und Katalogeintrag gehören in die erste Aufgabe einer Funktion (Gerüst, Opus), damit
  Oberfläche und MCP-Tool danach parallel entstehen können.
- **Skill `torro-api`**: Regeln für neue Befehle und Tools (Namen, Bereiche, Zugriff, Fehler,
  Tool-Beschreibungen, Tests). Die Coder-Agenten laden ihn, wenn eine Aufgabe einen Request anfasst.
- **Agent `torro-api-review`**: prüft nach jeder Funktion, ob Oberfläche, API und MCP
  übereinstimmen und die Sicherheitsregeln eingehalten sind, so wie `torro-design-review` das
  Design prüft. Er ändert keinen Code.
- Eigene Coder-Agenten für API und MCP gibt es nicht: Die Arbeit ist dank Katalog klein und
  mechanisch, die vorhandenen `torro-coder-*` erledigen sie mit dem Skill.

## Entscheidungen

Getroffen am 2026-10-11.

1. **HTTP-API später.** Sie kommt erst, wenn es einen ersten Nutzer gibt (Aufgabe A.10).
2. **App-Schlüssel als Datei** (`app.key` im State-Verzeichnis, 0600), weil jede App ihn beim Start
   braucht und der Schlüsselbund unter Linux beim ersten Zugriff nachfragen kann. Sicherheit gleich
   (siehe „Was der Schlüssel schützt“).
3. **Admin für Agenten freischaltbar**: eigener Schalter je Bereich, Standard aus, und jeder
   `Admin`-Befehl eines Agenten braucht immer eine Freigabe.
4. **Überschreiben ist Löschen**: `update_template` und `load_template` mit `replace` brauchen
   `Delete`.
5. **Ein-Konto-Requests entfallen**: `Account` und `DisconnectAccount` gehen in `Accounts` und
   `RemoveAccount` auf.
6. **Ein Name je Befehl**: Mit dem Umschlag (A.1) trägt `method` den Katalognamen
   (`calendar.list_calendars`); die alten Draht-Namen (`calendar-collections`) entfallen.
7. **Volle Schemata**: `Event`, `Todo`, `Settings` und `LoadMode` bekommen JSON-Schemata
   (`schemars`), damit Agenten wissen, welche Felder nötig sind (A.0c).
8. **Tray-Befehle**: „Settings…“ öffnet die Steuerzentrale über `apps.open_app` (auch für API
   und MCP); „Quit“ wird `system.quit`, nur App-Schlüssel (siehe „Was nie nach außen geht“).

Getroffen am 2026-10-19 (#231):

9. **API und MCP getrennt in der Oberfläche**, ein Client-Modell im Kern: Keys der Art `script` unter API,
   MCP-Clients unter MCP, gleicher Rechte-Editor. Verworfen: getrennte Client-Modelle (doppelte Prüfung).
10. **Sperrliste je Befehl** (`Rights::denied`) unter der Stufe statt Schalter allein. Kein neuer Befehl:
    Sie reist mit `clients.update_client` in den ganzen Rechten.
11. **Weg im Umschlag** (`via`) und die Schalter `allow_api_keys`, `allow_mcp`; einschalten nur Menschen.
12. **Rechte von Assistent und Bots im Assistent-Fenster**; der Kern gibt für die Arten `assistant` und `bot`
    keinen Schlüssel heraus (`clients.create_client` antwortet für `assistant` ohne Key, `clients.new_key` ist für
    beide `unsupported`), damit kein Schlüssel in einen App-Prozess gelangt.
13. **„Other MCP client“** (Art `mcp`) hat wie ein Skript einen eindeutigen Namen, der seine Quelle im Eingang ist
    (`agent:mcp:<name>`).
