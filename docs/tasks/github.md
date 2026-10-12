# Aufgaben: mehrere Konten und GitHub

Konzept: [github.md](../github.md); mehrere Konten aus [mail.md](../mail.md) (dort „M1“). Regeln:
[CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`. Wer einen Request anfasst, lädt
`torro-api`; nach jeder Funktion prüft `torro-api-review`.

Reihenfolge: **M1** (Grundlage) → **P** (Aufgaben-Modell, gilt auch für Nextcloud) → **G** (GitHub).
**K.21** (Aufgaben im Kalender) ist unabhängig und kann jederzeit laufen.

```
M1.1 ─┬─ M1.2
      ├─ M1.3
      ├─ M1.4
      └─ G.1 ── G.2 ── G.2b ─┬─ G.3 ─┬─ G.4
                             │       └─ G.5
P.1 ── P.2 ─┬─ P.3a ─────────┼─ P.3b (nach dem Redesign)
            └────────────────┴─ G.6
K.21 (frei)
```

## M1 Mehrere Konten

### M1.1 Kern: Konten-Liste, PIM je Konto
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-core/src/account.rs`,
  `crates/torroworkplace/src/{account,pim,core,settings_service,clients}.rs`, `crates/torro-pim/src/{account,source,demo,lib}.rs`,
  `crates/torro-ipc/src/{lib,pim,api}.rs`, `crates/torro-gui/src/source.rs`, Aufrufer von `PimSource::account` in
  `torrocalendar`/`torrotasks` (nur anpassen, keine neue Oberfläche)
- Der Kern hält **alle** Konten aus `accounts.json`, nicht mehr das erste: `PimService` bekommt je Konto
  einen eigenen Zustand (Verbindung, Sync, Problem, Sammlungen, Termine, Aufgaben); Sync läuft je Konto,
  ein Fehler bei einem Konto lässt die anderen unberührt.
- **Kennungen mit Konto:** Sammlungen heißen `<account-id>/<url>`. Schreiben und Löschen finden das Konto
  über die Kennung. Ausgeblendete Sammlungen in `settings.json` und Auswahlen in `clients.json`, die noch
  alte Kennungen (nackte URLs) enthalten, werden beim Laden einmal umgeschrieben (es gab bisher genau ein
  Konto, also eindeutig) und atomar gespeichert.
- `accounts.connect_nextcloud` fügt ein **weiteres** Konto hinzu (dasselbe Konto zweimal: vorhandenes
  ersetzen); `accounts.remove_account { id }` entfernt nur dieses; `accounts.sync` gleicht alle ab.
- **Schnittstelle (am 2026-10-11 mit M1 freigegeben):** `PimSource::account()` wird `accounts()` und liefert
  eine Liste (Kennung, Anzeigename, Art, `AccountState`). Genaue Form hier festlegen und im Bericht nennen.
  `DemoSource` bekommt zwei Konten, damit M1.3 etwas zu zeigen hat.
- **Oberfläche:** keine neue (M1.2, M1.3). **API:** keine neuen Requests, nur geändertes Verhalten; Katalog-
  beschreibungen von `accounts.*` und den Sammlungs-Kennungen anpassen. **MCP:** dieselben Tools.
- Abnahme: Tests für zwei Konten (getrennter Sync, Fehler in einem, Schreiben ans richtige Konto, Entfernen
  eines Kontos), Umschreiben alter Kennungen in Einstellungen und Rechten, Rechte-Filter mit neuen Kennungen.

### M1.2 Steuerzentrale: mehrere Konten
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** M1.1 · **Dateien:** `crates/torroworkplace/src/ui/{accounts,wizard,overview}.rs`
- „Add account“ ist immer da, auch wenn schon ein Konto besteht. Der Assistent beginnt mit der Wahl des
  Anbieters (vorerst nur Nextcloud; der Schritt entfällt, solange es nur einen gibt). Liste aller Konten mit
  Status je Konto; Detailseite und „Remove account…“ wirken auf genau ein Konto. Übersicht zählt alle Konten.
- **API/MCP:** nutzt `accounts.list_accounts`, `accounts.connect_nextcloud`, `accounts.remove_account` (gibt es).
- Abnahme: Sichtprüfung mit zwei Konten (Breit, Mittel, Schmal), Tests der Nachrichten.

### M1.3 Kalender und Aufgaben: Konto-Gruppen
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** M1.1 · **Dateien:** `crates/torrocalendar/src/app.rs`, `crates/torrotasks/src/app.rs`,
  ggf. `crates/torro-gui/src/shell.rs` (nennen)
- Ab zwei Konten gruppiert die Seitenleiste Kalender bzw. Listen je Konto (`sidebar_group_in`, Kontoname als
  Überschrift, einklappbar). Mit einem Konto sieht alles aus wie heute. Smarte Listen und „alle Kalender“
  bleiben kontoübergreifend.
- Abnahme: Sichtprüfung mit `--demo` (zwei Konten), mit einem Konto unverändert.

### M1.4 Konten absichern (aus der API-Prüfung von M1.1)
- [x] erledigt
- **Modell:** Opus · **Braucht:** M1.1 · **Dateien:** `crates/torroworkplace/src/{pim,account,core,clients,settings_service}.rs`,
  `crates/torro-pim-caldav/src/{client,write,nextcloud}.rs`, `crates/torro-pim/src/account.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torro-core/src/i18n/de.rs` (nur die Meldung zur Fernlöschung)
- **Geheimnis nie an fremde Server (hoch):** `place`/`State::owner` lösen Schreibziele nur über eine **exakte**
  Kennung aus den zwischengespeicherten Sammlungen auf, sonst `not-found` (heute reicht der Präfix: Suite-Request
  `<anna>/<URL eines anderen Hosts>` schickt Annas App-Passwort dorthin). Zusätzlich lehnt der CalDAV-Client jede
  URL ab, deren Origin nicht der seines Servers ist (`request`, PUT/DELETE, `home.join(href)` beim PROPFIND).
- **Anmeldung kann kein Konto übernehmen (mittel):** In `finish` muss der Origin von `credentials.server` zum
  Start-Server passen, sonst ablehnen. Bei erneuter Anmeldung am selben Konto das alte App-Passwort nach
  Möglichkeit widerrufen. Kennungen, die mit `/https:` o. Ä. einen Präfix-Trick in `account_of` erlauben, gibt
  es dann nicht mehr; zusätzlich Zuordnung exakt (siehe oben).
- **Fehlercodes und Beschreibungen:** `accounts.remove_account` mit unbekannter Kennung → `not-found` (Doc-Kommentar
  nennen). `calendar.save_event`/`tasks.save_todo`: „Changing `collection` moves it to another calendar of the same
  account; moving to another account fails with `unsupported`.“ (Katalog und Doc-Kommentar).
- **Robust laden:** Ein Schlüsselbund-Fehler oder ein fehlendes Geheimnis bei einem Konto bricht das Laden der
  anderen nicht ab; das Konto erscheint in `accounts.list_accounts` mit Problem („Sign in again“) und lässt sich
  entfernen.
- **Alte Kennungen nur einmal umschreiben:** `UpdateClient` und `UpdateSettings` lehnen nackte URLs ab (`error`),
  damit eine alte Auswahl nicht später still dem ersten Konto zufällt.
- Meldung zur Fernlöschung nennt das Konto (`trf("… {account} …")`, Übersetzung in `de.rs`).
- **Oberfläche:** keine neue. **API/MCP:** keine neuen Requests; Beschreibungen wie oben.
- Abnahme: Test mit zwei Fake-Servern (Suite schreibt auf `<anna>/<URL von bob>` → `not-found`, an keinen Server
  geht etwas); Origin-Prüfung im Client; Anmeldung mit abweichendem `server` → abgelehnt; `remove_account`
  unbekannt → `not-found`; Konto ohne Geheimnis erscheint mit Problem und lässt sich entfernen; nackte URL in
  `UpdateClient` → `error`.

## P Aufgaben-Modell (auch für Nextcloud)

### P.1 CalDAV behält fremde Eigenschaften
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-pim-caldav/src/{ical,write,client}.rs`,
  `crates/torroworkplace/src/pim.rs`
- Heute schreibt `to_ics_todo`/`to_ics_event` das Element beim Speichern komplett neu: Was das Modell nicht
  kennt (`X-APPLE-SORT-ORDER`, `CATEGORIES`, `URL`, `VALARM`, `X-*` von Nextcloud Tasks und anderen
  Programmen), geht verloren. Künftig behält der Kern den gelesenen Text je Element und setzt beim Speichern
  nur die Eigenschaften, die das Modell kennt, in die vorhandene Komponente ein; alles andere bleibt.
- Kein neuer Request. Abnahme: Rundreise-Tests mit Fixtures aus Nextcloud Tasks und Thunderbird (unbekannte
  Eigenschaften und `VALARM` bleiben, geänderte Felder sind neu).

### P.2 Modell: Link, Kategorien, Reihenfolge
- [x] erledigt
- **Modell:** Opus · **Braucht:** P.1, **Freigabe der Schnittstelle** (offener Punkt 1 in `github.md`)
- **Dateien:** `crates/torro-pim/src/{model,demo}.rs`, `crates/torro-pim-caldav/src/{ical,write}.rs`,
  `crates/torro-ipc/src/api.rs` (Beschreibungen), API-Referenz
- `Todo` bekommt `url: Option<String>` (`URL`), `categories: Vec<String>` (`CATEGORIES`),
  `sort_order: Option<i64>` (`X-APPLE-SORT-ORDER`) und `link: Option<Link>` (`X-TORRO-LINK`, z. B.
  `github:mahype/TorroWorkplace#123`, plus die Angaben, die der Kern von GitHub dazu holt). Alle mit
  `#[serde(default)]`, damit ältere Clients weiterlaufen.
- **Oberfläche:** Aufgaben-Detail zeigt Kategorien als Chips und einen Link als „Open link“.
  **API/MCP:** über `tasks.save_todo`/`tasks.list_todos` (gibt es), Schema der Tools wächst mit.
- Abnahme: Lesen/Schreiben der neuen Eigenschaften, alte JSON ohne die Felder.

### P.3a Reihenfolge von Hand: Kern, API, MCP
- [x] erledigt
- **Modell:** Opus · **Braucht:** P.2 · **Dateien:** `crates/torro-ipc/src/{lib,api,pim}.rs`, `crates/torro-pim/src/source.rs`,
  `crates/torro-pim/src/demo.rs`, `crates/torro-gui/src/source.rs`, `crates/torroworkplace/src/{pim,core,mcp}.rs`,
  `crates/torro-core/src/settings.rs` (Sortierart je Liste)
- Gerüst der Funktion (Request und Katalog zuerst, damit P.3b die Oberfläche darauf baut). Je Liste eine Sortierart:
  „Group by due date“ (wie heute, Standard) oder „Manual order“. Verschieben schreibt `X-APPLE-SORT-ORDER` nur bei
  den Aufgaben, deren Platz sich ändert (wie Nextcloud Tasks: kleinere Zahl steht oben; Lücken nutzen, nur bei
  Bedarf neu nummerieren).
- **API:** `Request::MoveTodo` · `tasks.move_todo` `{ todo: href + etag, before: Option<href> }` · Write; wie
  `save_todo` an `href`/`etag` erkannt, `before` muss in derselben Liste liegen (sonst `not-found`), veraltete
  ETags → `conflict`; Antwort: die geänderten Aufgaben. `out_of_reach` prüft beide. `PimSource` bekommt
  `move_todo` (Demo und Socket-Client). Sortierart über `settings.set_setting` (neuer `SettingKey`,
  Katalogbeschreibung ergänzen; Admin wie alle Einstellungen). **MCP:** `tasks_move_todo`.
- **Oberfläche:** P.3b.
- Nachtrag aus der API-Prüfung: `list_order` prüft die Reichweite gegen die für den Client **sichtbaren
  Aufgabenlisten** (`PimService::sees_task_list`), nicht gegen die Auswahl allein; versteckte Listen und
  Kalender-Kennungen sind `not-found` (auch bei `Selection::All`), `set_list_order` nimmt nur bekannte Aufgabenlisten.
  Katalogtexte von `tasks.move_todo` und `list_order` präzisiert.
- Abnahme: Tests für Einsortieren (Anfang, Mitte, Ende, Lücken, Neu-Nummerieren), `conflict`, fremde Liste,
  Rechte (Client ohne die Liste → `not-found`), MCP-Tool baut den richtigen Request.

### P.3b Reihenfolge von Hand: Oberfläche
- [x] erledigt
- **Modell:** Opus · **Braucht:** P.3a, Redesign der Aufgaben-App · **Dateien:** `crates/torrotasks/src/{app,list}.rs`
- In „Manual order“ Griff beim Hover im linken Innenrand, Ziehen mit Platzhalterzeile, Alt+↑/↓; Liste › Menü
  „Display“ › „Order“. Nutzt `tasks.move_todo` und `settings.set_setting` aus P.3a.
- Abnahme: Sichtprüfung, Tests der Tastatur.

## G GitHub

### G.1 GitHub-Konto
- [x] erledigt
- **Modell:** Opus · **Braucht:** M1.1, M1.2 · **Dateien:** `crates/torro-core/src/account.rs`, `crates/torroworkplace/src/{account,core,mcp}.rs`,
  `crates/torroworkplace/src/ui/{accounts,wizard}.rs`, `crates/torro-ipc/src/{lib,api}.rs`
- `AccountKind::GitHub`. Assistent: „Use GitHub CLI sign-in“ (wenn `gh auth token` etwas liefert, einmal
  bestätigen) oder „Paste a token“ (Link zur Token-Seite mit vorausgewählten Rechten). Prüfen mit der
  GraphQL-Abfrage `viewer { login }`, Rechte aus `X-OAuth-Scopes` bzw. einer Testabfrage; Token in den
  Schlüsselbund, Kennung `github:<login>`. Detailseite zeigt Login, Rechte und was ohne sie fehlt
  („Closing issues needs write access to issues“).
- **Oberfläche:** Steuerzentrale › Add account › GitHub. Die Oberfläche liest nichts selbst: `gh auth token`
  ruft der Dienst auf.
- **API:**
  - `Request::ConnectGithub` · `accounts.connect_github` · Admin: übernimmt die Anmeldung der GitHub CLI
    (Dienst ruft `gh auth token`), prüft und speichert; ohne `gh`-Anmeldung `not-connected` mit Hinweis.
  - `Request::SetGithubToken { token }` · `accounts.set_github_token` · Admin, **nur App-Schlüssel**: der
    eingefügte Token. Eigener Typ, der sich in `Debug` schwärzt; nie in Freigaben, `audit.jsonl` oder
    Fehlermeldungen.
- **MCP:** `accounts_connect_github` öffnet den Assistenten beim Menschen (`torroworkplace ui --section accounts`)
  und antwortet „opened“; `accounts.set_github_token` ist `Mcp::Never` (Geheimnis, api.md Ausnahme 1).
- Abnahme: Tests mit aufgezeichneten Antworten (kein Netz), Token nie in Antworten, `Debug` oder Log;
  Rechte-Test: `set_github_token` mit Client-Schlüssel → `forbidden`.

### G.1b Hinweise aus der API-Prüfung von G.1
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** G.1 · **Dateien:** `crates/torroworkplace/src/ui/wizard.rs`, `crates/torroworkplace/src/core.rs`,
  `crates/torroworkplace/src/account.rs`, `crates/torro-ipc/src/{server,api}.rs`
- `Message::TokenChanged` trägt den Token als `Secret` (oder eigenes `Debug`), damit kein abgeleitetes `Debug`
  den Klartext zeigt.
- `accounts.connect_github` für Clients: Ist die Steuerzentrale schon offen, wechselt sie auch zur Kontoseite
  (wie die Katalogbeschreibung verspricht).
- `gh` auch finden, wenn der Dienst unter launchd/systemd ohne vollen PATH läuft: bekannte Orte als Rückfall
  (`/opt/homebrew/bin`, `/usr/local/bin`, `~/.local/bin`, `~/bin`; Windows: Programme-Verzeichnis), sonst Meldung,
  dass `gh` nicht gefunden wurde, mit dem Hinweis auf den Token-Weg.
- `torro-ipc/src/server.rs`: „malformed request“ ohne die serde-Meldung (sie kann Werte zitieren, z. B. einen
  Token im falschen Feld), wie schon beim Schlüssel.
- **Oberfläche/API/MCP:** keine neuen Requests.
- Abnahme: Tests für `Debug` der Message, Suche nach `gh` (Rückfall-Pfade mit Temp-Verzeichnis), feste Meldung bei
  kaputtem Request.

### G.2 Filterlisten im Kern
- [x] erledigt
- **Modell:** Opus · **Braucht:** G.1, P.2 · **Dateien:** `crates/torro-pim-github/` (neu), Wurzel-`Cargo.toml`,
  `crates/torro-pim/src/model.rs` (`Collection::source`), `crates/torroworkplace/src/{pim,core,mcp,clients}.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`
- Eine Filterliste = Name, Farbe, eine oder mehrere GitHub-Suchanfragen (ODER), angeheftete Issues. Ablage
  `filter-lists.json` im Config-Verzeichnis, je Konto. Eingaben dürfen Suchanfragen **oder github.com-Adressen**
  sein (`/issues?q=…`, `/pulls?q=…`, Organisation, globale Suche); der Kern macht daraus die Anfrage und setzt
  `repo:`/`org:` aus dem Pfad, wenn die Anfrage keins hat.
- Abfrage per GraphQL `search(type: ISSUE)` + `nodes` (Angeheftete); jede Liste wird ganz neu abgefragt (Start,
  alle 5 Minuten, `accounts.sync`). Abbildung auf `Todo`: Titel, Text, geschlossen → erledigt, Sub-Issues →
  `parent`, Labels → `categories`, `url`, `link` (Typ Issue/PR, Repo, Nummer, Status, Label-Farben, Zuständige,
  Meilenstein; bei PRs Entwurf, Review-Stand, CI, Konflikt). Meilenstein-Datum → `due`. Mehr als 1 000 Treffer:
  Hinweis im Zustand der Liste.
- **API** (alle im neuen Bereich `Area::Github`):
  - `Request::CreateFilterList` · `tasks.create_filter_list` · Write
  - `Request::UpdateFilterList` · `tasks.update_filter_list` · Write; trägt den Versionsstempel der Liste,
    veraltet → `conflict` (zwei Clients überschreiben sich nicht)
  - `Request::DeleteFilterList` · `tasks.delete_filter_list` · Delete
  - `Request::PreviewFilter` · `tasks.preview_filter` (Anzahl und erste Treffer, ohne zu speichern) · Read
  - `Request::UnpinIssue` · `tasks.unpin_issue { collection, url }` · Write
- **MCP:** `tasks_create_filter_list`, `tasks_update_filter_list`, `tasks_delete_filter_list`, `tasks_preview_filter`,
  `tasks_unpin_issue`. **Oberfläche:** G.3, G.4.
- **Rechte** (Konzept: „Rechte für Agenten“ in `github.md`; Schnittstellen-Änderung, Freigabe nötig):
  `Rights` bekommt `github_repos: Selection` (Repos und Organisationen), `Area` bekommt `Github`. Für Clients
  filtert der Kern **jeden Treffer** (Vorschau, Filterliste, Angeheftete) gegen `github_repos`, bevor er gelesen
  wird; zusätzlich gilt `task_lists` für die Listen selbst. Neue Listen kommen nie von selbst in eine Auswahl
  „nur diese“, Kennungen werden nie wiederverwendet. `out_of_reach` in `pim.rs` behandelt jeden Request
  ausdrücklich (kein `_`-Zweig). Steuerzentrale › Agents & API zeigt die Repo-Auswahl (Seite nennen, falls
  schon vorhanden; sonst Folgeaufgabe anlegen).
- **Protokoll:** nur Kennungen (Listen-Kennung, `owner/repo#n`), nie Anfragen oder Titel.
- Katalogbeschreibungen nennen die Nebenwirkungen; `tasks.list_lists` sagt, dass Filterlisten `writable: false`
  sind und Abhaken trotzdem geht; `accounts.sync` gleicht Filterlisten mit ab. API-Referenz neu erzeugen.
- Abnahme: Tests für Adressen → Anfrage (Fixtures), Abbildung aus aufgezeichneten GraphQL-Antworten, Wegfallen
  nicht mehr passender Einträge, Angeheftete; Rechte: Vorschau und umgeschriebene Anfrage zeigen einem Client
  nur Treffer aus `github_repos`, fremde Liste → `not-found`, neue Liste nicht in `Only`-Auswahl.

### G.2b Filterlisten: Seitenkanäle schließen (aus der API-Prüfung von G.2)
- [x] erledigt
- **Modell:** Opus · **Braucht:** G.2 · **Dateien:** `crates/torroworkplace/src/{pim,core}.rs`, `crates/torro-pim-github/src/{address,lists,lib}.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`
- **Prüfen vor dem Fragen (hoch):** Für einen Client mit eingeschränkter Repo-Auswahl (`github_repos` = nur diese)
  prüft der Kern jede Suchanfrage, **bevor** GitHub gefragt wird: Jede Anfrage braucht mindestens ein
  `repo:`/`org:`/`user:` aus der Auswahl und kein positives `repo:`/`org:`/`user:` außerhalb; sonst `forbidden`
  mit fester Meldung. Gilt für `preview_filter`, `create_filter_list`, `update_filter_list`. Heute sucht GitHub
  mit dem vollen Token und es wird erst danach gefiltert: Fehlercode, `matches`, `truncated` und Antwortzeit
  verraten dann Namen und Trefferzahlen fremder (auch privater) Repos. Das Nachfiltern bleibt als zweite Sicherung.
- **Feste Meldungen für Clients:** GitHub-Fehlermeldungen gehen an Clients nicht wörtlich hinaus (auch nicht in
  `source.problem`), sondern als feste Meldung je Code.
- **Listen nur sichtbar, wenn sie ganz in der Auswahl liegen (mittel):** Eine Filterliste ist für einen Client nur
  sichtbar (`sees`, `out_of_reach`), wenn alle ihre Anfragen nach der Regel oben in seiner Auswahl liegen; sonst
  verhält sie sich wie nicht vorhanden. Sonst liest er in `source.queries` die Namen privater Repos.
- **Antworten filtern (mittel):** `answer_collection` nach `create`/`update` wendet `reach.collection_for` an
  (heute enthält `source.pinned` dort fremde Einträge).
- **Hinweise:** `preview_filter`-Doc nennt `not-connected`; Vorschau für Clients begrenzen (weniger Seiten oder
  `busy` bei zu vielen Aufrufen in kurzer Zeit), damit ein Agent das Rate-Limit nicht aufbraucht;
  `settings.set_collection_shown` prüft `sees_task_list` wie `ListOrder`; Katalogtexte: `create_filter_list` sagt,
  dass eine neue Liste nicht in eine eingeschränkte Listen-Auswahl kommt, `list_lists`/`list_todos` sagen, dass
  Einträge, `pinned` und `parent` fremder Repos fehlen.
- **Oberfläche/MCP:** keine neuen Requests.
- Abnahme: Tests: Client mit `Only([mahype/a])` → Anfrage mit `repo:mahype/secret` bzw. ohne `repo:` → `forbidden`,
  **an GitHub geht nichts** (Fake-Server zählt Aufrufe); Liste eines Menschen mit fremdem Repo für den Client
  unsichtbar; `update` durch Client liefert kein fremdes `pinned`; feste Meldungen; `set_collection_shown`.

### G.3 Aufgaben-App: Filterlisten zeigen
- [x] erledigt
- **Modell:** Opus · **Braucht:** G.2b, R.2, R.3 (`docs/tasks/design.md`) · **Dateien:** `crates/torrotasks/src/{app,list,detail}.rs`,
  `crates/torro-gui/src/shell.rs`, `crates/torro-pim/src/{account,demo}.rs` (Beispiel-Filterliste)
- Zeilen und Detail für Issues und PRs baut schon das Redesign (R.2, R.3). G.3 bringt die **Filterlisten** in die App:
  Seitenleiste mit Gruppe des GitHub-Kontos und eigenem Symbol je Filterliste; Gruppierung nach Repo als Standard in
  Filterlisten; Hinweise der Liste aus `source` (Problem beim Abruf, „Showing the first 1,000“); angeheftete Einträge
  mit „Unpin“ im „…“-Menü (`tasks.unpin_issue`).
- **API/MCP:** liest über `tasks.list_lists`/`tasks.list_todos`; Lösen über `tasks.unpin_issue` (gibt es).
- **Fund aus G.1:** `shell::account_groups` zählt ein GitHub-Konto ohne Listen mit (eine Nextcloud + GitHub zeigt
  dann eine überflüssige Konto-Überschrift), und `torro_pim::account::combined_state` hält eine reine
  GitHub-Einrichtung für „verbunden“ (die Kalender-App zeigt dann nicht „No calendar account yet“). Nur Konten
  mit Sammlungen der jeweiligen Art zählen.
- Abnahme: Sichtprüfung `--demo` in allen Größenklassen (`torro-design-review`).

### G.3b Filterlisten: Kleinigkeiten aus G.3
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** G.3 · **Dateien:** `crates/torrotasks/src/{app,list,detail}.rs`, `crates/torro-gui/src/icon.rs`
  (+ `assets/icons/pin-off.svg`), ggf. `crates/torroworkplace/src/pim.rs` (Reihenfolge)
- **Dasselbe Issue in zwei Filterlisten:** Der Kern nimmt die Link-Kennung als `uid`; die App findet Cursor und
  offenes Detail nur über `uid` und kann so auf der falschen Kopie landen. Cursor/Detail über (`collection`, `uid`).
- **Reihenfolge in Filterlisten** wie die Sortierung der Suche (`sort:updated-desc` usw.), nicht nach
  Fälligkeit/Priorität/Titel (`docs/github.md`, „Anzeige“) – der Kern liefert die Treffer in Suchreihenfolge,
  `sort_order` oder ein Index trägt sie zur App.
- **Symbol für „Unpin“:** Lucide `pin-off` (0.469.0) statt X.
- **Oberfläche/API/MCP:** keine neuen Requests.
- Abnahme: Test mit demselben Issue in zwei Listen (Cursor, Detail), Test der Reihenfolge, Sichtprüfung.

### G.4 Dialog: Filterliste anlegen und bearbeiten
- [x] erledigt
- **Modell:** Opus · **Braucht:** G.3 · **Dateien:** `crates/torrotasks/src/{app,filter}.rs` (`filter.rs` neu)
- „New filter list…“ und „Edit filter…“: Feld zum Einfügen (Anfrage oder Adresse), darunter das Formular
  (Typ, Repos, Status, Labels mit/ohne, Zuständig, Autor, Erwähnt, Review angefragt, Meilenstein, Sortierung;
  vorerst **Textfelder**, eine Auswahl aus GitHub bräuchte einen eigenen Lese-Request),
  das die Anfrage schreibt; weitere Anfragen hinzufügen; Vorlagen; Vorschau mit Anzahl. „Delete list…“ mit
  Rückfrage („The issues stay on GitHub.“).
- **API:** `tasks.create_filter_list`, `tasks.update_filter_list`, `tasks.delete_filter_list`, `tasks.preview_filter`
  (aus G.2). **MCP:** dieselben Tools.
- Abnahme: Tests Formular ↔ Anfrage (Rundreise, unbekannte Teile bleiben stehen), Sichtprüfung.

### G.5 Abhaken schließt das Issue
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** G.2, G.3 · **Dateien:** `crates/torro-pim-github/`, `crates/torroworkplace/src/pim.rs`,
  `crates/torrotasks/src/{app,list}.rs`
- `tasks.save_todo` mit `completed` auf einem Issue einer Filterliste schließt es („completed“) bzw. öffnet es
  wieder; andere Felder sind dort nicht änderbar (`unsupported`). PRs: kein Haken. Ohne Schreibrecht: Haken aus,
  Hinweis. In der App Meldung „Issue closed“ mit „Undo“.
- **Oberfläche:** Haken in der Liste. **API:** `tasks.save_todo` (gibt es). Katalogbeschreibung ergänzen: „Marking
  a GitHub issue, or a link to one, done closes the issue on GitHub; pull requests are never closed.“ Für
  Clients nur bei Repos in `github_repos` und Stufe `write` im Bereich `github`. **MCP:** `tasks_save_todo`.
- Abnahme: Tests mit aufgezeichneten Antworten (schließen, öffnen, PR abgelehnt, fehlendes Token-Recht);
  Rechte: Client ohne das Repo bzw. ohne `github`-Schreiben → `not-found` bzw. `forbidden`.

### G.5b Freigabe sagt „Close GitHub issue“ (aus der API-Prüfung von G.5)
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** G.5 · **Dateien:** `crates/torroworkplace/src/{approvals,pim,core}.rs`, `crates/torro-pim-github/src/graphql.rs`
- **Fehler (mittel):** Braucht `tasks.save_todo` auf einem Issue einer Filterliste eine Freigabe, zeigt sie heute
  „Change task "…" in "…"“ (`approvals.rs` `todo_saved`, Facts aus `pim.rs` ~1414) – nicht, dass ein Issue auf
  GitHub geschlossen bzw. wieder geöffnet wird, und mit dem `summary` aus der Client-Eingabe, obwohl eine
  Titeländerung dort nie liefe. Die Zusammenfassung muss zeigen, was wirklich passiert: Facts bekommen das
  Merkmal „GitHub-Issue“ mit `owner/repo#n`; Text „Close GitHub issue "{title}" ({repo}#{n})“ bzw. „Reopen …“,
  Titel aus dem Cache, ohne Client-`summary`.
- Test: Freigabe offen → GitHub-Schreibrecht entzogen → zugestimmt ⇒ `forbidden`, an GitHub geht nichts.
- `graphql.rs` `change_error`: im Doc-Kommentar festhalten, dass nur `errors[0]` gelesen wird.
- **Oberfläche/API/MCP:** keine neuen Requests. Neue Texte englisch mit `tr()` (Freigaben sieht der Mensch).
- Abnahme: Tests für beide Texte (schließen/öffnen), Client-`summary` taucht nicht auf, Entzug vor Zustimmung.

### G.6 Verweise: Issues in eigene Listen holen
- [x] erledigt
- **Modell:** Opus · **Braucht:** G.2, P.2, P.3a · **Dateien:** `crates/torroworkplace/src/{pim,core,mcp}.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torrotasks/src/{app,list,detail}.rs`
- Link in der Schnelleingabe: in einer Filterliste → anheften; in einer schreibbaren Liste → **Verweis** (VTODO
  mit `URL`, `X-TORRO-LINK`, Titel von GitHub). In einer Filterliste „Add to list…“ im Detail und Ziehen auf
  eine Liste der Seitenleiste. Der Kern gleicht Verweise beim Sync ab: Titel und Status von GitHub; Issue
  geschlossen → Verweis erledigt. Haken an einem Verweis auf ein Issue schließt auch das Issue (mit „Undo“), bei
  einem PR nur den Verweis. Datum, Priorität, Reihenfolge und Unteraufgaben gehören dem Verweis.
- **API:** `Request::AddLink` · `tasks.add_link { collection, url }` · Write (Antwort: der neue Eintrag).
  Für Clients: Zielliste in `task_lists`, Repo des Links in `github_repos` (sonst `not-found`); Abhaken eines
  Verweises schließt das Issue nur unter denselben Bedingungen. Protokoll nur mit `owner/repo#n`.
  **MCP:** `tasks_add_link`.
- Abnahme: Tests für Erkennen von Links, Anheften vs. Verweis, Abgleich (Titel, geschlossen), Haken bei Issue
  und PR; Rechte: Link auf ein Repo außerhalb der Auswahl → `not-found`, Verweis darauf schließt nichts;
  Sichtprüfung einer gemischten Liste in „Manual order“.

### G.6b Verweise absichern (aus der API-Prüfung von G.6)
- [x] erledigt
- **Modell:** Opus · **Braucht:** G.6 · **Dateien:** `crates/torroworkplace/src/{pim,core,approvals}.rs`, `crates/torro-ipc/src/{api,lib}.rs`,
  `crates/torro-pim-caldav/src/{client,nextcloud}.rs` (Besitz einer Liste), `crates/torrotasks/src/app.rs` (Rückmeldung), `docs/github.md`
- **Leselücke (hoch):** Ein Client ohne GitHub-Recht, aber mit `github_repos = All` (Vorlagen), speichert per
  `tasks.save_todo` eine Aufgabe mit fremdem `link`; `reconcile_links` schreibt mit dem vollen Token Titel und Zustand
  hinein, der Client liest sie über `list_todos` (`pim.rs` ~689, `links_elsewhere` ~2150 prüft nur `reaches_repo`).
  Fix: Jeder neu gesetzte oder geänderte Link wird abgewiesen, wenn der Client GitHub nicht lesen darf (wie bei
  `add_link`); Test mit `Level::NoAccess` + `Selection::All`.
- **Geteilte Nextcloud-Listen:** Wer eine mit mir geteilte Liste beschreiben kann, könnte dort ein `X-TORRO-LINK`
  ablegen und so Titel meiner privaten Issues auslesen. Der Abgleich löst Verweise nur in Listen auf, die dem Konto
  selbst gehören (Nextcloud: Besitzer der Sammlung = angemeldeter Benutzer; geteilte Listen nicht), oder in denen
  der Verweis von dieser Suite angelegt wurde – Weg festlegen und begründen. In `docs/github.md` festhalten.
- **Freigabe für `tasks.add_link`** (`approvals.rs` ~865, heute „Run tasks.add_link“): eigener Text wie bei
  `UnpinIssue` – „Pin {item} to filter list "{list}"“ bzw. „Add a link to {item} to "{list}"“, Liste aus dem Cache,
  `owner/repo#n` aus `address::issue_ref`, kein Client-Text. Unlesbare `url` schon vor der Freigabe mit `error` ablehnen.
- **Freigabe beim Abhaken eines Verweises** (`approvals.rs` ~1063): Die Kurzform „Close GitHub issue …“ nur, wenn sich
  außer `completed` nichts ändert; sonst „Close GitHub issue … and change task …“ mit den übrigen Änderungen.
- **Katalogtexte:** `tasks.add_link` nennt die nötigen Rechte (GitHub lesen; zum Anheften GitHub schreiben, sonst
  `forbidden`) und `not-connected`; `tasks.save_todo`: bei Verweisen ohne Schreibrecht wird nur die Aufgabe abgehakt,
  ohne Fehler; Zeile ≤ 120 Zeichen.
- **App:** kurze Rückmeldung nach „Add to list…“ („Added to {list}“).
- **`docs/github.md`, Offene Punkte:** Clients ohne GitHub-Lesen sehen Titel/Zustand von Verweisen, die der Mensch
  angelegt hat (eigene Aufgaben, bewusst); bei mehreren GitHub-Konten nimmt der Kern das erste mit Token
  (später: Konto im Link mitführen).
- **Oberfläche/API/MCP:** keine neuen Requests.
- Abnahme: Tests für die Leselücke (an GitHub geht nichts, nichts wird in die Liste geschrieben), geteilte Liste wird
  nicht abgeglichen, beide Freigabetexte (inkl. Abhaken mit weiteren Änderungen), `error` vor Freigabe.

### G.6c Verweise: geteilte Listen, Ziehen auf die Seitenleiste
- [x] erledigt
- **Modell:** Opus · **Braucht:** G.6b, G.3b · **Dateien:** `crates/torroworkplace/src/{pim,core,approvals}.rs`,
  `crates/torrotasks/src/{app,list}.rs`, `crates/torro-gui/src/shell.rs` (falls die Seitenleiste ein Ziel braucht), `docs/github.md`
- **Geteilte Listen (entschieden 2026-10-12, sichere Voreinstellung):** In Listen, die nicht privat sind (G.6b:
  `Slot::private`), erledigt der Haken an einem Verweis **nur die Aufgabe**; das Issue bleibt offen. Sonst könnte
  jemand, mit dem die Liste geteilt ist, einen Verweis unterschieben, und mein Haken schlösse ein Issue. Freigabetext
  und Katalogtext entsprechend; in `docs/github.md` festhalten.
- **Ziehen auf die Seitenleiste (Rest aus G.6):** Einen Eintrag einer Filterliste auf eine schreibbare Liste der
  Seitenleiste ziehen legt dort einen Verweis an (`tasks.add_link`); eine Aufgabe auf eine andere Liste desselben
  Kontos ziehen verschiebt sie (`tasks.save_todo` mit neuer Liste). Ziel beim Ziehen hervorheben. Nutzt das Ziehen
  aus P.3b.
- **Oberfläche/API/MCP:** keine neuen Requests.
- Abnahme: Tests (Haken in geteilter Liste schließt nichts, an GitHub geht nichts; Ziehen legt Verweis an bzw.
  verschiebt), Sichtprüfung des Ziehziels.

## Neue Liste: Aufgabenliste oder GitHub-Filterliste

Entschieden 2026-10-12: Statt „New filter list…“ je GitHub-Konto gibt es **einen** Eintrag „New list…“. Er fragt
zuerst, was entstehen soll: **Task list** (eigene Aufgaben in Nextcloud) oder **GitHub filter list**. Die
Filterliste beginnt mit dem **Repository**; Labels, Meilensteine und Personen kommen aus dem Repo und werden
angeklickt statt getippt. Das zuletzt gewählte Repo ist vorausgewählt, **für alle Konten gemeinsam**. Zur Auswahl
stehen nur Repos, die das angemeldete GitHub-Konto erreicht (keine freie Suche über ganz GitHub). Jedes
Bedienelement zeigt deutlich, dass es gedrückt und gewählt ist. Reihenfolge: zuerst die Aufgabenliste (G.7, G.8),
dann die GitHub-Seite (G.9–G.11).

Entwurf (klickbar, Ablauf und Zustände): https://claude.ai/artifact/DTcLrfy15MY9vesEQXySaX – Aufbau des Dialogs:

- **Auswahl** (560 breit): zwei Karten „Task list“ / „GitHub filter list“ mit Icon, Satz darunter, Tasten 1 / 2.
- **Task list** (520 breit): Konto (nur bei mehreren Nextcloud-Konten, als Chips), Name (Fokus), Farbe (5 Punkte),
  Vorschau der Seitenleistenzeile, Fuß „Cancel“ / „Create list“. Zurück-Pfeil oben links.
- **GitHub filter list** (960 breit, Formular links, Vorschau 300 rechts): Repository (Auswahlfeld mit Suche,
  „Last used“), Start from (Vorlagen-Chips), Show und State (Segmente), Labels (Chips mit Farbpunkt, Mehrfachwahl,
  Häkchen), Assignee (Anyone / Me / Unassigned / Personen), Milestone (Any / … / No milestone), „More filters“
  aufklappbar (Author, Review requested, Sort, Without labels), Trennlinie, Name (vorgeschlagen aus den Filtern,
  „Suggested from your filters“) und Farbe, „GitHub search“ aufklappbar (die geschriebene Anfrage, von Hand
  änderbar, Einfügen von Anfrage oder Adresse, weitere Anfrage). Vorschau: „7 matches“, erste fünf Treffer,
  „Checking…“ während gefragt wird, leerer Zustand mit Rat.
- **Nach dem Anlegen:** Dialog zu, neue Liste in der Seitenleiste gewählt, Meldung „List "{name}" created“.

### G.7 Aufgabenliste anlegen: Kern, API, MCP
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-pim-caldav/src/{client,write}.rs`,
  `crates/torro-pim/src/source.rs`, `crates/torroworkplace/src/{pim,core,mcp,approvals}.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`; zusätzlich geändert: `crates/torro-ipc/src/pim.rs`,
  `crates/torro-pim/src/demo.rs`, `crates/torro-gui/src/source.rs`, `crates/torroworkplace/src/{audit,daemon}.rs`.
  Offen: `docs/api-reference.md` (entsteht mit A.9).
- Neue Aufgabenliste in einem Nextcloud-Konto: CalDAV `MKCALENDAR` im Kalender-Zuhause des Kontos mit
  `displayname`, `calendar-color` (Apple-Namensraum, `#RRGGBBFF` wie Nextcloud) und
  `supported-calendar-component-set` = nur `VTODO`. Der Pfad ist ein neuer, nie wiederverwendeter Name
  (Kleinbuchstaben aus dem Namen plus Zufallsteil); besteht er schon (`405`/`409`), neuer Versuch, kein
  Überschreiben. Danach ist die Liste sofort in `tasks.list_lists` (Cache aktualisieren, kein Warten auf den Sync).
- **API:** `Request::CreateTaskList { account, name, color }` · `tasks.create_list` · Write · Bereich `Tasks`.
  Antwort: die neue `Collection`. Fehler: `not-found` (Konto unbekannt oder für den Client unsichtbar),
  `unsupported` (Konto ohne CalDAV, z. B. GitHub), `offline`, `auth`, `error` (leerer Name).
- **MCP:** `tasks_create_list`.
- **Oberfläche:** G.8.
- **Rechte:** Clients brauchen `write` im Bereich `tasks`; die neue Liste kommt nie von selbst in eine Auswahl
  „nur diese“ (`task_lists`), wie bei Filterlisten. Braucht der Befehl eine Freigabe, lautet sie
  „Create task list "{name}" in {account}“ (Konto-Name aus dem Kern, nicht vom Client). Protokoll nur mit
  der Konto-Kennung (der Pfad enthält einen Teil des Namens), nie mit dem Namen.
- Katalogbeschreibung nennt die Nebenwirkung („Creates a new task list on the account's server …“). API-Referenz
  offen (entsteht mit A.9).
- Nicht Teil: Umbenennen, Farbe ändern, Löschen einer Aufgabenliste (eigene Aufgabe, wenn gewünscht).
- Abnahme: Tests mit aufgezeichneten CalDAV-Antworten (Anfrage-Körper, Pfad-Kollision, Fehler), Handler-Test
  (Liste danach in `list_lists`), Rechte (Client ohne `write` → `forbidden`, neue Liste nicht in `Only`-Auswahl),
  MCP-Tool baut den richtigen Request.

### G.8 Oberfläche: „New list…“ mit Auswahl und Aufgabenliste
- [x] erledigt
- **Modell:** Opus · **Braucht:** G.7 · **Dateien:** `crates/torrotasks/src/{app,filter}.rs`,
  `crates/torrotasks/src/new_list.rs` (neu), ggf. `crates/torro-gui/src/widgets.rs` (Auswahlkarte)
- Ein Eintrag „New list…“ (Icon `Plus`) unter allen Listen der Seitenleiste und im Navigationsmenü, statt
  „New filter list…“ je GitHub-Konto. Öffnet die Auswahl (siehe Entwurf oben); Tasten 1 / 2 wählen, Escape
  schließt. „GitHub filter list“ öffnet den Dialog aus G.4 (bis G.11 ihn ersetzt); ohne GitHub-Konto ist die Karte
  aus und sagt „Connect a GitHub account in Settings first.“; ohne Nextcloud-Konto ebenso für „Task list“.
- Dialog „New task list“: Konto (nur bei mehreren), Name, Farbe, Vorschau der Zeile; „Create list“ schickt
  `tasks.create_list`, zeigt solange „Creating…“ mit Drehsymbol, schließt dann, wählt die neue Liste und meldet
  „List "{name}" created“. Fehler unter dem Feld bzw. als Meldung mit „Retry“. Breit/Mittel über dem Fenster,
  Schmal als eigene Ansicht (wie der Filterlisten-Dialog).
- **API/MCP:** `tasks.create_list` aus G.7; die Auswahl selbst ist reine Oberfläche.
- Übersetzungen im Bericht nennen.
- Abnahme: Tests (Auswahl → richtiger Dialog, Request mit Konto/Name/Farbe, neue Liste gewählt), Sichtprüfung in
  allen Größen hell und dunkel.

### G.9 GitHub: Repos, Labels, Meilensteine und Personen lesen
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** G.2 · **Dateien:** `crates/torro-pim-github/src/{api,graphql}.rs`,
  `crates/torro-pim/src/source.rs`, `crates/torroworkplace/src/{pim,core,mcp}.rs`, `crates/torro-ipc/src/{lib,api}.rs`;
  zusätzlich geändert: `crates/torro-ipc/src/pim.rs`, `crates/torro-pim/src/demo.rs`, `crates/torro-gui/src/source.rs`,
  `crates/torroworkplace/src/{audit,daemon}.rs`. Offen: `docs/api-reference.md` (entsteht mit A.9).
- Die Lese-Requests, die G.4 noch fehlten („eine Auswahl aus GitHub bräuchte einen eigenen Lese-Request“):
  - `Request::ListGithubRepos { account }` · `tasks.list_github_repos` · Read · Bereich `Github`: die Repos, die
    das angemeldete Konto erreicht (GraphQL `viewer.repositories`, Zugehörigkeit Eigentümer, Mitarbeiter,
    Organisationsmitglied; nicht archiviert), neueste Änderung zuerst, je Repo `owner/name`, privat ja/nein.
  - `Request::DescribeGithubRepo { account, repo }` · `tasks.describe_github_repo` · Read · Bereich `Github`: in
    **einer** Abfrage Labels (Name, Farbe), offene Meilensteine (Titel) und zuweisbare Personen (Login) des Repos.
- **MCP:** `tasks_list_github_repos`, `tasks_describe_github_repo`. **Oberfläche:** G.11.
- **Rechte:** Für Clients filtert der Kern die Repo-Liste gegen `github_repos`; ein Repo außerhalb → `not-found`
  (wie nicht vorhanden). Der Token verlässt den Kern nie. Protokoll nur mit Konto und `owner/repo`.
- Antworten kurz zwischenspeichern (bis zum nächsten Abgleich), damit der Dialog nicht bei jedem Öffnen fragt.
- Abnahme: Tests aus aufgezeichneten GraphQL-Antworten (Fixtures), Rechte-Fälle, MCP-Tools bauen die Requests.

### G.9b Aufgabenliste und GitHub-Lesen absichern (aus der API-Prüfung von G.7 und G.9)
- [x] erledigt
- **Modell:** Opus · **Braucht:** G.7, G.9 · **Dateien:** `crates/torro-pim-caldav/src/write.rs`,
  `crates/torroworkplace/src/{pim,core}.rs`, `crates/torro-pim-github/src/lib.rs`, `crates/torro-ipc/src/api.rs`,
  `docs/tasks/github.md`
- **403 auf `MKCALENDAR` ist kein `auth` (mittel):** `made` reicht heute alles außer 405/409 an `status_error`, dort
  wird 403 zu `ProviderError::Auth`, und `PimService::failed` startet die Remote-Wipe-Prüfung („sign in again“).
  Ein 403 heißt hier aber „an dieser Stelle darf keine Liste entstehen“, das Passwort gilt. Fix: in `made` nur 401
  als `Auth`, 403 als `ProviderError::Other("the server does not allow new task lists here (HTTP 403)")`.
- **Zwischenspeicher: Rennen und verwaister Slot (mittel):** `list_github_repos` und `describe_github_repo`
  schreiben nach der GitHub-Abfrage über `filter_slot_mut`, das einen fehlenden `FilterSlot` neu anlegt. Wird das
  Konto währenddessen entfernt, bleibt ein verwaister Slot; läuft ein Refresh dazwischen, steht die ältere Antwort
  wieder drin. Fix: Generation je Slot (fortlaufend im Kern, neu bei jedem Refresh und jedem neuen Slot); vor der
  Abfrage merken, nur zurückschreiben, wenn der Slot noch da ist (suchen, nicht anlegen) und die Generation gleich.
- **`describe`-Speicher begrenzen (niedrig):** höchstens 64 Repos je Konto, das älteste fällt heraus; ein Agent mit
  `github_repos: All` kann sonst beliebig viele öffentliche Repos beschreiben lassen. Katalogtext von
  `tasks.describe_github_repo`: jedes Repo, das der Token lesen kann, nicht nur die aus `list_github_repos`.
- **Gleiche Meldung (niedrig):** Ein Repo außerhalb der Auswahl eines Clients antwortet wie eines, das es nicht gibt:
  `no repository "x"` (`no_repo`), nicht `no collection "x"`.
- **Kleinigkeiten (niedrig):** verrutschter Doc-Kommentar über `GITHUB_READ_FAILED` zurück an `no_filter_list`;
  `GITHUB_READ_FAILED` nach `torro-pim-github` neben `GITHUB_FAILED`/`CHANGE_FAILED`, `read_message` gebaut wie
  `change_message` (Ergebnis unverändert); `tasks.list_github_repos` nennt „at most 1,000 repositories“.
- **Doku:** G.7 und G.9 berichtigt (Protokoll, zusätzlich geänderte Dateien, `docs/api-reference.md` offen bis A.9).
- **Oberfläche/API/MCP:** keine neuen Requests; nur Katalogtexte.
- **Später (nicht Teil):** Sync-Sperre je Konto statt global während `create_task_list`; optionales
  `truncated: bool` in den Antworten von `list_github_repos`/`describe_github_repo`.
- Abnahme: Tests – 403 auf `MKCALENDAR` ist `error` und löst keine Wipe-Prüfung aus (401 schon); Konto entfernt
  bzw. Refresh während der Abfrage → nichts zurückgeschrieben, kein Slot angelegt; Grenze 64 mit dem ältesten
  heraus; Rechte-Test vergleicht die Meldung `no repository …`.

### G.10 Rückmeldung: Chips, Segmente und Knöpfe zeigen ihren Zustand
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** – · **Dateien:** `crates/torro-gui/src/{style,widgets}.rs`,
  `crates/torro-gui/examples/gallery.rs`
- Befund vom Nutzer: Im Filterlisten-Dialog merkt man beim Klicken nicht, dass etwas passiert. Zuerst per
  Screenshot klären, welche Bausteine keinen Hover-, Gedrückt- oder Gewählt-Zustand zeigen (Chips der Vorlagen,
  Auswahllisten, Segmente).
- Bausteine nach Skill `torro-design` Abschnitt 5 und dem Entwurf (Brett „States and feedback“): wählbarer Chip
  (`widgets::choice_chip(label, mark, on, on_press)`: gewählt = `SELECTED`, stärkerer Rand, Häkchen), Mehrfachwahl
  mit Farbpunkt für Labels, Segment mit deutlichem aktivem Feld, Primärknopf mit Zustand „busy“ (Drehsymbol, Text
  „Creating…“, nimmt keine zweiten Klicks), Fokusring auf allen. Auswahl bleibt neutral, nie Rot.
- **Oberfläche/API/MCP:** keine neuen Requests.
- Abnahme: Galerie zeigt alle Zustände hell und dunkel; `scripts/design-check.sh` sauber.

### G.11 Dialog „GitHub filter list“ neu: Repo zuerst, Werte zum Anklicken
- [ ] erledigt
- **Modell:** Opus · **Braucht:** G.8, G.9, G.10 · **Dateien:** `crates/torrotasks/src/{app,filter}.rs`,
  `crates/torro-gui/src/ui_state.rs` (zuletzt gewähltes Repo)
- Aufbau wie im Entwurf oben. Die Anfrage bleibt die Wahrheit (G.4): das Formular schreibt nur die Wörter, die es
  kennt, alles andere bleibt stehen; die Rundreise-Tests aus G.4 bleiben grün.
- **Repo zuerst:** Auswahlfeld mit Suche aus `tasks.list_github_repos`; vorausgewählt das zuletzt gewählte Repo,
  gespeichert in `ui.json` (`torro_gui::ui_state`, wie die Seitenleiste) **ohne Konto**, also für alle Konten
  gleich; erreicht das gewählte Konto es nicht, das erste Repo der Liste. Repo wechseln entfernt Labels und
  Meilenstein, die es dort nicht gibt.
- **Werte zum Anklicken** aus `tasks.describe_github_repo`: Labels (mit / ohne) als Chips mit Farbpunkt,
  Meilenstein und Personen als Chips; Typ, Status, Sortierung als Segmente; Author, Review requested, Sort und
  Without labels unter „More filters“ (aufgeklappt, wenn etwas davon gesetzt ist). Lädt es noch: ruhige
  Platzhalter; scheitert es: Hinweis mit „Retry“, das Suchfeld bleibt benutzbar.
- **Name** wird aus den Filtern vorgeschlagen („Bugs · TorroWorkplace“, Vorlagenname · Repo), bis man tippt.
- **Bearbeiten** („Edit filter…“) nutzt denselben Aufbau; hat die Liste mehrere Anfragen oder mehrere Repos in
  einer Anfrage, ist „GitHub search“ aufgeklappt und das Repo-Feld zeigt „Several repositories“.
- **API/MCP:** nur vorhandene Requests (G.2, G.9); das zuletzt gewählte Repo ist eine Einstellung der Oberfläche
  wie der Zustand der Seitenleiste.
- Übersetzungen im Bericht nennen.
- Abnahme: Tests (Repo vorausgewählt und gemerkt, Chips ↔ Anfrage, Repo-Wechsel, Vorschlag des Namens,
  Bearbeiten mit mehreren Repos), Sichtprüfung in allen Größen hell und dunkel.

## K Kalender

### K.21 Fällige Aufgaben im Kalender
- [ ] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torrocalendar/src/{app,views/*}.rs`, `crates/torro-core/src/settings.rs`
- Aufgaben mit Fälligkeit erscheinen als ganztägige Einträge mit Haken-Symbol (mit Uhrzeit: an ihrer Zeit, als
  schmale Markierung). Abhaken direkt im Kalender, Klick öffnet die Aufgabe im Paneel. Je Liste ein-/ausschaltbar
  (Seitenleiste des Kalenders, Gruppe „Task lists“), Standard: an. Gilt für Nextcloud-Listen, Verweise und
  Filterlisten gleich.
- **Oberfläche:** Kalender. **API:** liest `tasks.list_todos`, schreibt `tasks.save_todo` (gibt es); Auswahl der
  Listen über `settings.set_setting` (neuer Schlüssel, Katalogbeschreibung ergänzen). **MCP:** `settings_set_setting`.
- Abnahme: Sichtprüfung Tag, Woche, Monat, Agenda; Tests für Einordnen (Datum, Zeit, überfällig).
