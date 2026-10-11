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
      └─ G.1 ── G.2 ─┬─ G.3 ─┬─ G.4
                     │       └─ G.5
P.1 ── P.2 ─┬─ P.3a ─┼─ P.3b (nach dem Redesign)
            └────────┴─ G.6
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
- [ ] erledigt
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
- Abnahme: Tests für Einsortieren (Anfang, Mitte, Ende, Lücken, Neu-Nummerieren), `conflict`, fremde Liste,
  Rechte (Client ohne die Liste → `not-found`), MCP-Tool baut den richtigen Request.

### P.3b Reihenfolge von Hand: Oberfläche
- [ ] erledigt
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

### G.2 Filterlisten im Kern
- [ ] erledigt
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

### G.3 Aufgaben-App: Filterlisten zeigen
- [ ] erledigt
- **Modell:** Opus · **Braucht:** G.2 · **Dateien:** `crates/torrotasks/src/{app,list,detail}.rs`, `crates/torro-gui/src/icon.rs`,
  `crates/torro-pim/src/demo.rs` (Beispiel-Filterliste)
- Seitenleiste: Filterlisten in der Gruppe des GitHub-Kontos mit eigenem Symbol. Zeile: Typ-Symbol (Issue
  offen/geschlossen, PR Entwurf/offen/gemergt), Titel, `Repo#123`, Labels als dezente Chips, Fälligkeit; bei
  PRs Review-Stand und CI. Gruppierung nach Repo. Detail im Paneel: Text als Markdown (nur lesen), Labels,
  Zuständige, Meilenstein, Hauptknopf „Open on GitHub“.
- **API/MCP:** liest über `tasks.list_lists`/`tasks.list_todos` (gibt es).
- **Fund aus G.1:** `shell::account_groups` zählt ein GitHub-Konto ohne Listen mit (eine Nextcloud + GitHub zeigt
  dann eine überflüssige Konto-Überschrift), und `torro_pim::account::combined_state` hält eine reine
  GitHub-Einrichtung für „verbunden“ (die Kalender-App zeigt dann nicht „No calendar account yet“). Nur Konten
  mit Sammlungen der jeweiligen Art zählen.
- Abnahme: Sichtprüfung `--demo` in allen Größenklassen (`torro-design-review`).

### G.4 Dialog: Filterliste anlegen und bearbeiten
- [ ] erledigt
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
- [ ] erledigt
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

### G.6 Verweise: Issues in eigene Listen holen
- [ ] erledigt
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
