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
| `accounts` | `list_accounts` | `sync` | | `connect_nextcloud`, `remove_account` |
| `settings` | `get_settings` | | | `set_setting`, `set_collection_shown`, `update_settings` |
| `apps` | `list_apps` | `open_app` | | |
| `clients` | | | | `list_clients`, `create_client`, `update_client`, `new_key`, `remove_client` (**nur App-Schlüssel**, siehe unten) |
| `approvals` | `list_approvals`, `approval_status` | | | `decide_approval` (**nur App-Schlüssel**) |

¹ `load_template` braucht `Write`, mit `mode: replace` aber `Delete`, weil es offene Fenster
schließt. Die Stufe hängt also vom Request ab, nicht nur vom Namen (`Request::operation()`).
`update_template` überschreibt eine Vorlage unwiederbringlich und ist deshalb `Delete`.

Die Ein-Konto-Requests `Account` und `DisconnectAccount` entfallen; es gibt nur noch
`accounts.list_accounts` und `accounts.remove_account`. Die Liste der Apps (`apps.list_apps`)
ermittelt künftig der Dienst, nicht mehr die Steuerzentrale.

`approvals` hat wie `system` keine Stufe in den Rechten: Jeder gültige Schlüssel liest seine
**eigenen** Freigaben (ein Agent muss erfahren können, wie sein Befehl ausging), der App-Schlüssel
alle. Entscheiden kann nur der App-Schlüssel.

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
   auch nicht über den App-Schlüssel. Die API sagt nur „gespeichert“ oder „fehlt“.
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
   („Delete event "Team meeting" on Tue, 14 Oct“).
3. Der Mensch sieht sie als **Benachrichtigung mit „Allow“/„Deny“**, als Zahl am Tray-Icon und im
   Abschnitt „Approvals“ der Steuerzentrale. Nur der App-Schlüssel kann entscheiden.
4. Das MCP-Tool wartet bis zu 60 Sekunden auf die Entscheidung und gibt dann das Ergebnis zurück.
   Dauert es länger, antwortet es „waiting for approval“ mit der Kennung; der Agent fragt später
   mit `approvals_approval_status` nach.
5. Bei „Allow“ prüft der Dienst Rechte und Reichweite des Clients **zum Zeitpunkt der
   Entscheidung** erneut und führt den Befehl dann aus; die Antwort steht danach in der Freigabe.
   Entschiedene und abgelaufene Freigaben bleiben einen Tag lesbar. Ein Client kann höchstens zehn
   Freigaben gleichzeitig offen haben (sonst `busy`), damit er den Menschen nicht mit
   Benachrichtigungen überschüttet. Befehle aus „Was nie nach außen geht“ werden nie zu einer
   Freigabe.

Das ist zugleich der Anfang des „Freigaben-Eingangs“ aus dem [App-Katalog](app-catalog.md).

## Protokoll

- `audit.jsonl` im State-Verzeichnis, **0600**: je Anfrage eines fremden Clients Zeit, Client,
  Befehl, betroffene Kennungen, Ergebnis. **Keine Inhalte** (keine Titel, keine Suchbegriffe);
  TorroMail schreibt Suchbegriffe und Empfänger mit und legt die Datei lesbar für alle ab.
- Die Karte eines Clients zeigt „zuletzt benutzt“ und die letzten Aktionen; „Diagnostics“ zeigt
  das ganze Protokoll. Die Datei wird bei 5 MB rotiert.
- Anfragen des App-Schlüssels werden nicht protokolliert (das wäre jede Bewegung in der Oberfläche).

## Transport

- **Jetzt:** `core.sock` (Unix-Socket, 0600; Windows: Named Pipe, kommt mit GlazeWM), eine
  JSON-Zeile je Richtung, Umschlag mit `v` und `key`. `ping` meldet die API-Version.
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
- MCP-Toolnamen sind stabil wie API-Namen.

## Steuerzentrale: Abschnitt „Agents & API“

Ersetzt „AI & agents“ aus [settings.md](settings.md).

- **Liste der Clients als Karten**: Name, Art (Claude Code, Codex, Claude Desktop, Cursor, Script),
  Vorlage der Rechte als Wort, „zuletzt benutzt vor …“, Status nur bei Abweichung.
- **„Connect agent“** öffnet einen Assistenten:
  1. **Welcher Client:** Liste der bekannten Clients; erkannt, welche installiert sind.
  2. **Rechte:** Vorlage wählen, Kalender/Listen auswählen.
  3. **Verbinden:** Der Schlüssel wird direkt in den Client eingetragen (Claude Code über
     `claude mcp add -s user -e TORRO_TOKEN=… torroworkplace -- torroworkplace mcp`, Codex über
     seine CLI, JSON-Konfigurationen atomar mit 0600), oder bei „Script“ einmal angezeigt zum
     Kopieren. Danach ist er nie wieder sichtbar.
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
