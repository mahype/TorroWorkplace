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
| `workspaces` | `list_templates` | `save_template`, `rename_template`, `load_template` ¹ | `update_template`, `delete_template` | |
| `calendar` | `list_calendars`, `list_events` | `save_event` | `delete_event` | |
| `tasks` | `list_lists`, `list_todos` | `save_todo` | `delete_todo` | |
| `inbox` | `list_items`, `get_item`, `list_sources`, `list_rules` | `post_item`, `resolve_items`, `set_item_done`, `snooze_item`, `set_item_lane` ², `open_item` ³, `create_task_from_item` ⁴ | | `mute_source`, `remove_rule`, `set_item_lane` mit `remember` ² |
| `accounts` | `list_accounts` | `sync` | | `connect_nextcloud`, `remove_account` |
| `settings` | `get_settings` | | | `set_setting`, `set_collection_shown`, `update_settings` |
| `apps` | `list_apps` | `open_app` | | |
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
`not-found`), vor der Freigabe und bei ihrer Ausführung noch einmal.

⁵ `ask_permission { tool, details, project? }` (E2.5) stellt dem Menschen eine Rückfrage „darf der
Agent dieses Werkzeug benutzen?“, wie sie der Hook `PermissionRequest` von Claude Code und Codex
stellt. Sie ist eine **Freigabe eigener Art** (siehe „Freigaben“): Sie wartet immer auf den Menschen,
gleich was die Rechte sagen, läuft aber schon nach **32 Sekunden** ab (30 s Wartezeit des Hooks und
etwas Spielraum) statt nach zehn Minuten. „Allow“ führt im Dienst nichts aus, sondern beantwortet nur
die Rückfrage (Zustand `done` mit `ok`, sonst `denied` bzw. `expired`). Jeder Schlüssel darf fragen,
auch ein Hook-Client; er liest mit `approval_status` nur seine **eigenen** Rückfragen, sonst weiterhin
nichts (Regel 4). Die Zusammenfassung baut der Dienst: „Claude Code wants to use Bash in
"my-project": cargo test“, Fragender bei Hook-Clients der Agent, sonst der Name des Clients, das
Projekt in Anführungszeichen. `details` (der genaue Befehl) erscheint **ungekürzt**, nur ohne Steuer-
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
Hook, der eine Antwort annimmt.

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
  gekürzt, höchstens 16 `pids`. Nur `http`/`https`-Links und `app`-Links auf Apps der Suite werden
  gespeichert, andere verworfen.
- Den `thread_key` eines Eintrags sieht nur, wer ihn gemeldet hat (und der App-Schlüssel); bei
  fremden Einträgen ist er leer.
- Die Freigabe eines `inbox.*`-Befehls nennt Eintrag, Regel und Quelle aus dem Speicher des Eingangs
  (`approvals::inbox_summary`, nachgeschlagen über die Kennung), nie aus dem Text des Clients.
  `open_item` nennt, was sich öffnet (Host, App oder Fenster des Agenten), `create_task_from_item`
  Titel, Liste und Fälligkeit.
- Eine Freigabe für einen Eintrag merkt sich, wie er beim Fragen aussah (`updated` und Zahl der
  Meldungen, `approvals::EntryPin`). Hat der Client bis zur Ausführung in denselben Faden neu
  gemeldet, antwortet der Dienst `conflict` und führt nichts aus (E1.2c). `create_task_from_item`
  ohne Titel bekommt beim Fragen den Titel des Eintrags fest in die Anfrage geschrieben.

Die Ein-Konto-Requests `Account` und `DisconnectAccount` entfallen; es gibt nur noch
`accounts.list_accounts` und `accounts.remove_account`. Die Liste der Apps (`apps.list_apps`)
ermittelt künftig der Dienst, nicht mehr die Steuerzentrale.

`approvals` hat wie `system` keine Stufe in den Rechten: Jeder gültige Schlüssel liest seine
**eigenen** Freigaben (ein Agent muss erfahren können, wie sein Befehl ausging) und darf eine
Rückfrage stellen (`ask_permission`), der App-Schlüssel liest alle. Entscheiden kann nur der
App-Schlüssel.

`set_collection_shown { collection, shown }` ersetzt das Umschalten über `update_settings` in
Accounts. Es ist `Admin`, weil ausgeblendete Sammlungen auch für Agenten unsichtbar sind: Wer eine
Sammlung einblendet, erweitert, was Agenten sehen.

`Collections` liefert heute Kalender und Aufgabenlisten zusammen; die API teilt das nach Bereich
auf (`calendar.list_calendars`, `tasks.list_lists`), damit die Rechte sauber greifen. Die Präfixe
in [architecture.md](architecture.md) (`setups_*`) heißen künftig `workspaces_*`, wie in der
Oberfläche.

Neue Bereiche kommen mit ihren Funktionen dazu (`github`, `slack`, `contacts` …).

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
3. **Freigaben entscheiden** (`approvals.decide`): nur mit dem App-Schlüssel, also nur durch
   einen Menschen in der Oberfläche oder per Benachrichtigung.
4. **Den Dienst beenden** (`system.quit`): nur mit dem App-Schlüssel (Tray „Quit“, `ctl quit`).
   Ein Agent soll den Dienst nicht abschalten können, der ihn prüft und protokolliert.
5. **Geheimnisse entgegennehmen** (Token einfügen `accounts.set_github_token`, später Anmeldung mit
   Passwort, Import aus einem fremden Schlüsselbund): nur mit dem App-Schlüssel, also nur aus der
   Oberfläche. Solche Requests landen nie in einer Freigabe.

Allgemein gilt: Jeder Befehl mit `Mcp::Never` ist nur mit dem App-Schlüssel erlaubt. `Rights::allows`
setzt das durch (seit G.1), auch ein Script-Schlüssel mit Admin-Rechten bekommt `forbidden`.

Anmeldungen bei Anbietern (Login Flow v2, GitHub) gibt es über die API, sie enden aber immer im
Browser des Menschen; ein Agent bekommt nie ein Passwort zu sehen.

## Rechte

Agenten werden in der **Steuerzentrale, Abschnitt „Agents & API“**, verbunden, und dort wird je
Agent festgelegt, worauf er zugreifen darf (wie in TorroMail).

### Je Client

- **Je Bereich eine Stufe**: Kein Zugriff · Lesen · Lesen und Schreiben · Voll (mit Löschen).
  `Admin`-Befehle (Konten verbinden/entfernen, Einstellungen ändern) sind ein eigener Schalter je
  Bereich, Standard aus. Ist er an, braucht jeder `Admin`-Befehl trotzdem eine Freigabe durch den
  Menschen, unabhängig von „Freigabe vorher“.
- **Je Sammlung eine Auswahl**: „alle“ oder ausgewählte Kalender und Aufgabenlisten (später auch
  Repositories, Kanäle). Eine nicht freigegebene Sammlung verhält sich wie eine, die es nicht gibt;
  der Dienst filtert, bevor er Daten liest. Was die Suite ausblendet, sieht auch kein Agent.
- **Freigabe vorher**: Nie · Beim Löschen (Standard) · Bei jeder Änderung.
- **Vorlagen** für den schnellen Start: *Read only*, *Read and write* (Standard, Löschen mit
  Freigabe), *Full access*. Danach ist jede Zeile einzeln änderbar.

### Prüfung im Dienst

Für jede Anfrage: Schlüssel → Client → `Request::operation()` → Stufe im Bereich reicht? →
Sammlung freigegeben? → Freigabe nötig? → ausführen → protokollieren. Rechte werden bei der
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
3. Der Mensch sieht sie als **Benachrichtigung mit „Allow“/„Deny“**, als Zahl am Tray-Icon und im
   Abschnitt „Approvals“ der Steuerzentrale; bis dahin und auf Systemen ohne Knöpfe in
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

## Steuerzentrale: Abschnitt „Agents & API“

Ersetzt „AI & agents“ aus [settings.md](settings.md).

- **Liste der Clients als Karten**: Name, Art (Claude Code, Codex, Claude Desktop, Cursor, Script),
  Vorlage der Rechte als Wort, „zuletzt benutzt vor …“, Status nur bei Abweichung.
- **„Connect agent“** öffnet einen Assistenten:
  1. **Welcher Client:** Liste der bekannten Clients; erkannt, welche installiert sind.
  2. **Rechte:** Vorlage wählen, Kalender/Listen auswählen.
  3. **Verbinden:** Der Schlüssel wird direkt in den Client eingetragen (Claude Code über
     `claude mcp add -s user -e TORRO_TOKEN=… torroworkplace -- torroworkplace mcp`, Codex über
     seine CLI; Gemini CLI, Claude Desktop und Cursor über ihre JSON-Dateien), oder bei „Script“
     einmal angezeigt zum Kopieren. Danach ist er nie wieder sichtbar. Jede geänderte Datei wird
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
- **Detailseite eines Clients**: Rechte je Bereich (Zeilen mit Stufe), Sammlungen, Freigabe vorher,
  letzte Aktionen, „Create new key“ (alter wird ungültig), „Disconnect…“ (bestätigt; entfernt den
  Schlüssel aus dem Client, wenn möglich).
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
