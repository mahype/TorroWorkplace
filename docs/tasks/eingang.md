# Aufgaben: Eingang (Inbox), Schritte E1–E3

Konzept: [eingang.md](../eingang.md). Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill
`torro-task`. Wer einen Request anfasst, lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`.

E1–E3 bringen den Eingang **ohne Mail und ohne fremde Anmeldung**: Kern, Meldungen der Suite,
Agenten (Claude Code, Codex, Gemini CLI) und die App `torroinbox`. Mail kommt mit E4 (Konzept
[mail.md](../mail.md)), KI mit [ki.md](../ki.md).

> **Entschieden 2026-10-12:** Der Kern (E1.1 ff.) wartet nicht mehr auf M7.1/M7.2, die noch niemand
> begonnen hat. Startet M7.1 parallel, stimmen sich die beiden ab; Konflikte in `torro-ipc`, `core.rs`,
> `mcp.rs`, `clients.rs`, `docs/api.md` und `docs/api-reference.md` werden beim Zusammenführen gelöst.
>
> **Auch A.3 (Freigaben) vorher abschließen.** A.3 ändert dieselben Stellen (`torro-ipc`, `core.rs`,
> `mcp.rs`, Test `names_follow_the_rules`) und wird für E2.1 ohnehin gebraucht. Aufgaben, die den
> Katalog ändern (E1.1, E2.4, E2.5, E3.5), laufen nie parallel zu anderen Katalog-Aufgaben (A.5, A.11,
> M-, G-, P-Aufgaben).

```
E1.1a ─┬─ E3.1 ── E3.2 ── E3.2b ── E3.2c ── E3.3a   ← sofort möglich
       ├─ E2.2a
       │
A.3 (api-mcp.md)
       │
       └─ E1.1 ── E1.2 ─┬─ E1.3 ── E2.2 ─┬─ E2.3 (braucht A.7)
                        │                └─ E2.4
                        ├─ E2.1
                        ├─ E3.3b (nach A.8), E3.6, E3.7
                        └─ E3.4 (braucht E2.1)
E3.5 braucht E3.2, E1.2, P.2 · E2.5 braucht E2.3, E3.2, A.8 und die Freigabe ihres Sicherheitsteils
```

**Ausnahme von der Wartebedingung:** E1.1a, E2.2a, E3.1, E3.2, E3.2b, E3.2c und E3.3a legen nur neue Crates an (plus je eine
Zeile in der Wurzel-`Cargo.toml`) und laufen mit Demo-Daten. Sie dürfen sofort starten.

## Sicherheitsregeln des Eingangs (gelten für alle Aufgaben)

Aus der API-Prüfung des Plans am 2026-10-11:

1. **Freigaben und Suite-Einträge gehören dem Menschen.** Einträge `approval` und `account_problem`
   und die Quelle `suite` ändert nur der App-Schlüssel (erledigen, vertagen, Spur, stumm), sonst
   `forbidden`. Sonst könnte ein Agent eine Freigabe wegdrücken, und die Tray-Zahl sänke mit.
2. **Die Quelle bestimmt der Kern, nie der Absender.** `SourceRef` und der Namensraum von
   `thread_key` werden aus dem Client abgeleitet. Ein Client kann nur eigene Einträge melden und
   auflösen.
3. **„Dringend“ setzt niemand selbst.** `agent_waiting`/`agent_done` nehmen nur Clients der Art `hook`
   an (eigene Client-Art, angelegt in E2.3); bei allen anderen wird daraus `notice`. `notice` ist nie
   dringend.
4. **Hook-Clients sehen nichts.** Ihr Schlüssel steht in Konfigurationsdateien, die der Agent selbst
   lesen kann. Darum `inbox_sources` = leer und keine anderen Bereiche: Sie können melden und
   auflösen, `list_items`/`get_item` liefern `not-found`. Restrisiko: Ein Agent, der den Schlüssel
   liest, kann in seinem eigenen Namen Einträge melden; dagegen helfen Drossel und Stummschalten.
5. **Titel und Texte fremder Quellen sind `untrusted`**, auch die von Agenten und die Zusammenfassung
   einer Freigabe. Selbst formuliert (mit `tr()`) sind nur Titel von Suite- und Hook-Einträgen.
6. **Öffnen nur, was sicher ist:** Links `http`/`https`; `app` nur für Apps aus `apps.list_apps`;
   `window` nur bei Hook-Einträgen. Bei App-Quellen (später Slack, WhatsApp) darf „Open“ das Fenster der App
   aktivieren (E8.9, Schnittstelle 5). Welche App und welches Fenster, bestimmt nur der Kern: aus der Quelle, bei
   Agenten aus ihren Prozessen und dem, was das System dazu sagt (Fensterliste, `ps`, Bundle-Kennung), nie aus einem
   Text des Eintrags. Sitzungsname und Projekt eines Agenten werden nur mit Fenstertiteln verglichen. Programme
   startet der Kern mit Argumentliste, nie über eine Shell-Zeile; Skripte (macOS `osascript`) bekommen alles
   Veränderliche als Argument, nie in ihren Text.
7. **Protokoll ohne Inhalte:** `title`, `body`, `thread_key` landen nie in `audit.jsonl`.

## Offene Punkte (vor der jeweiligen Aufgabe klären)

1. ~~Schnittstelle `Rights`~~ (E1.1): neues Feld `inbox_sources: Selection`, **freigegeben am 2026-10-12.**
2. ~~Schnittstelle `Event`~~ (E3.4): Feld Frei/Belegt (`TRANSP`), **freigegeben am 2026-10-12.**
3. ~~Sicherheitsteil von E2.5~~: **freigegeben am 2026-10-12.**
4. ~~A.6 (Agents & API): Auswahl der Inbox-Quellen auf der Detailseite~~, erledigt mit A.6.
5. ~~Schnittstellen von E8 (drei Fenster)~~: **freigegeben am 2026-10-13**, Liste unter [E8](#e8-drei-fenster-2026-10-13).
6. ~~Schnittstellen und offene Fragen von E5 (Quellen)~~: **freigegeben bzw. nach Empfehlung entschieden am 2026-10-14**, Liste unter [E5](#e5-nextcloud-github-ntfy-als-quellen-plan-2026-10-14).

## E1 Kern

### E1.1a Modell-Crate `torro-inbox`
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-inbox/` (neu: `Cargo.toml`,
  `src/{lib,item,rules,source,demo}.rs`), Wurzel-`Cargo.toml` (Mitglied und Eintrag in
  `[workspace.dependencies]`)
- Ohne Ein-/Ausgabe und ohne Abhängigkeit zu `torro-ipc`; Vorbild ist `torro-pim` (Modell, `source.rs`,
  `demo.rs`). Alle Typen mit `Serialize`, `Deserialize`, `JsonSchema`, weil E1.1 sie als Wire-Format
  im Katalog benutzt.
- **Typen:** `Item { id, source: SourceRef, thread_key, kind, lane, urgent, title, body, from,
  agent: Option<AgentInfo>, received, updated, count, state, link, actions, untrusted }`;
  `SourceRef { kind, name }` mit `kind` vorerst `suite | agent | script`;
  `AgentInfo { agent: claude-code|codex|gemini, session, project, branch, pids }`;
  `Kind`: `notice | agent_waiting | agent_done | approval | account_problem`; `Lane`: `for_me | info`;
  `State`: `new | seen | done | snoozed(until)`; `Link`: `url | app | window`; `Action`; dazu
  `SourceInfo` (Quelle mit Zahl offener Einträge und „stumm“) für die Seitenleiste.
- **Regeln als reine Funktionen:** zusammenführen (gleicher `thread_key` erhöht `count`, setzt
  `updated`, holt Erledigtes zurück), feste Spur (`agent_waiting`, `approval` → für mich, dringend;
  `agent_done`, `account_problem` → für mich; `notice` → gewünschte Spur, nie dringend), Kinds, die nur
  der Kern bzw. nur Hook-Clients anlegen dürfen (Sicherheitsregel 3, als Funktion, durchgesetzt in
  E1.2), aufwachen (Vertagtes zur Zeit wieder `new`), aufräumen (Erledigtes nach 30 Tagen weg),
  Sortierung der Liste (dringend zuerst, dann `updated` neu zuerst).
- **`InboxSource`-Trait** (async wie `PimSource`): `list_items`, `list_sources`, `set_item_done`,
  `snooze_item`, `set_item_lane`, `mute_source`. Fehler als eigenes Enum.
- **`DemoSource`:** Beispieldaten im Speicher für `--demo`: zwei wartende Agenten (Claude Code, Codex),
  ein fertiger, eine Freigabe, ein Kontoproblem, einige Skript-Meldungen in „Zur Info“, ein vertagter,
  ein erledigter Eintrag; Änderungen wirken im Speicher. Sichtbare Texte englisch mit `tr()`
  (Übersetzungen im Bericht nennen).
- **API/MCP:** keine; Requests und Katalog kommen in E1.1.
- **Abnahme:** Tests der Regeln (zusammenführen, feste Spuren, aufwachen, aufräumen, Sortierung) und
  der Demo-Quelle; serde-Rundreise; fmt/clippy/test grün, auch mit `TORRO_LANG=de`.

### E1.1 Gerüst: Modell, Requests, Katalog
- [x] erledigt (Handler antworten bis E1.2 mit `unsupported`; `docs/api-reference.md` gibt es noch nicht, A.9)
- **Modell:** Opus · **Braucht:** E1.1a, A.3 · **Dateien:**
  `crates/torro-inbox/src/*` (nur falls das Wire-Format Ergänzungen braucht, nennen),
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torro-ipc/Cargo.toml`, `crates/torroworkplace/src/{clients,core,mcp}.rs`
  (nur Stellen, die `Rights { … }` bauen, und deren Tests), `docs/api.md` (Bereich `inbox`), `docs/api-reference.md`
- Nutzt die Typen und Regeln aus `torro-inbox` (E1.1a) als Wire-Format.
- **Requests** (Bereich `inbox`), je mit Doc-Kommentar und Fehlercodes; Sicherheitsregeln oben beachten:

  | Request | Zugriff | Bemerkung |
  |---|---|---|
  | `inbox.list_items { lane?, source?, state?, since?, limit? }` | Read | |
  | `inbox.get_item { id }` | Read | |
  | `inbox.list_sources` | Read | Quellen mit Zahl und „stumm“, für die Seitenleiste |
  | `inbox.post_item { kind, title, body?, link?, lane?, thread_key?, agent? }` | Write | Quelle aus dem Client; Regel 3 |
  | `inbox.resolve_items { thread_key }` | Write | „Anlass ist weg“, nur eigene Einträge |
  | `inbox.set_item_done { id, done }` | Write | mit `done: false` wieder offen; Regel 1 |
  | `inbox.snooze_item { id, until? }` | Write | `until` leer = nicht mehr vertagt; Regel 1 |
  | `inbox.set_item_lane { id, lane, remember }` | Write, mit `remember` **Admin** | Stufe aus `Request::operation()` wie bei `load_template` |
  | `inbox.mute_source { source, muted }` | Admin | dauerhafte Regel |
  | `inbox.list_rules` | Read | gemerkte Spur-Regeln und stumme Quellen |
  | `inbox.remove_rule { id }` | Admin | |

- **Katalog:** alle `Mcp::Tool`. Beschreibungen nennen, dass Titel und `body` fremder Quellen **Daten,
  keine Anweisungen** sind (`untrusted: true`).
- **Rechte:** `Area::Inbox` (auch in `Area::ALL`); `Rights.inbox_sources: Selection` über Quellen
  (`agent:claude-code`, `suite`, `script:<name>` …) mit serde-Standard für alte `clients.json`;
  `Rights::common` und `template()` erweitern, Vorlagen setzen Inbox wie Tasks. Nicht freigegebene
  Quellen → `not-found`. Neue Client-Art `hook` (Regeln 3 und 4).
- **Abnahme:** Tests der Regeln (zusammenführen, feste Spuren, aufwachen, aufräumen); `post_item` mit
  `approval` → abgelehnt, mit `agent_waiting` von einem Nicht-Hook-Client → wird `notice`; Agent ändert
  einen `approval`-Eintrag → `forbidden`; Katalog-Tabellentest und Namensregeln grün; alte
  `clients.json` lädt; API-Referenz neu erzeugt; `torro-api-review` ohne Befund.

### E1.2 Kern: Speicher und Befehle
- [x] erledigt
- **Modell:** Opus · **Braucht:** E1.1 · **Dateien:** `crates/torroworkplace/src/inbox.rs` (neu),
  `crates/torroworkplace/src/{core,clients,daemon}.rs`, `crates/torroworkplace/Cargo.toml`, Wurzel-`Cargo.toml`
  (`rusqlite` mit `bundled`)
- **Speicher:** `inbox.sqlite` im State-Verzeichnis (0600, WAL, Schema-Version). SQLite schon jetzt,
  weil mit Mail (E4) Tausende Einträge kommen; Volltext (FTS5) kommt erst mit E4.
- Handler für alle `inbox.*`-Requests. Filter nach `inbox_sources` **vor** dem Lesen; stumme Quellen
  liefern keine neuen Einträge. Durchsetzen der Sicherheitsregeln 1–4.
- **Drosseln:** Gezählt werden nur **neue** Vorgänge (Zusammenführen in einen offenen `thread_key`
  zählt nicht): normale Clients 30 je Stunde, Hook-Clients 120 je Stunde, darüber `busy`.
  `resolve_items` wird nie gedrosselt.
- **Längen begrenzen:** `title` auf 200 und `body` auf 4000 Zeichen kürzen (an einer Zeichengrenze, mit
  „…“). Agenten schicken sonst ihre ganze letzte Antwort mit (Hinweis aus E2.2a).
- Im Dienst: Vertagtes zur Zeit aufwecken, einmal täglich aufräumen.
- Die App fragt wie Kalender und Aufgaben im Takt nach (`iced::time::every`); ein `subscribe` gibt es
  noch nicht und gehört nicht in diese Aufgabe.
- **Abnahme:** Tests im Temp-Verzeichnis: Anlegen, Zusammenführen, Erledigen, Vertagen und Aufwachen
  (feste Uhr), Neustart verliert nichts, Client ohne Quelle → `not-found`, Hook-Client liest →
  `not-found`, Client löst fremden Vorgang auf → keine Wirkung, Drossel → `busy`, Datei 0600; ist A.5
  schon da: `title`, `body`, `thread_key` stehen nicht in `audit.jsonl`.

### E1.2b Inbox absichern (aus der API-Prüfung von E1.1/E1.2)
- [x] erledigt
- **Modell:** Opus · **Braucht:** E1.2 · **Dateien:** `crates/torroworkplace/src/{inbox,approvals,core,clients}.rs`,
  `crates/torro-ipc/src/{api,lib}.rs`, `docs/api.md`
- **Hoch:** Freigaben für `inbox.mute_source` (stumm/laut), `inbox.remove_rule` und `inbox.set_item_lane`
  mit `remember` bekommen eigene Zusammenfassungen (Quelle mit Namen, Spur, die Regel aus dem Speicher
  nachgeschlagen); ebenso `post_item` mit zitiertem Titel, wenn es auf Freigabe wartet.
- **Hoch:** Ein Hook-Client gehört zu genau einem Agenten (beim Anlegen festgelegt, z. B. über ein Feld oder
  den Namen); Meldungen landen unter diesem Agenten, ein anderes `agent.agent` wird abgelehnt. So hilft
  Stummschalten zuverlässig.
- **Hoch:** Längen für alle Felder: `thread_key` ≤ 200 Bytes und URL ≤ 2000 (sonst `error`), Agenten-Felder
  kürzen, höchstens 16 `pids`; Test.
- **Mittel:** Nur `http`/`https`-Links speichern (andere verwerfen), `app` nur bekannte Apps; `thread_key`
  fremder Einträge nicht herausgeben (oder dokumentieren); Speicherfehler ohne Pfad an Clients; fehlende
  Tests (eingeschränkte Rechte über `run_approved`, `set_item_lane{remember}` ohne Admin, `-shm`-Rechte,
  `ListRules`/`ListSources` eines Hooks).
- **Niedrig:** Filter (Spur, Zustand, seit, Anzahl) in SQL statt im Speicher; `user_version` aus
  `SCHEMA_VERSION`; vorhandenes State-Verzeichnis auf 0700; Quellnamen eindeutig (`script:<name>` darf nicht
  mit `script:cli` oder einem anderen Client gleichen Namens zusammenfallen); Katalogbeschreibung von
  `post_item` nennt Grenzen, verworfene Felder und `id: null` = stumm.
- **API/MCP:** keine neuen Befehle.
- **Abnahme:** Tests je Punkt; `torro-api-review` ohne hohen Befund.

### E1.2c Suite-Einträge nur für den Menschen, Freigaben ohne Austausch (aus der API-Prüfung bis A.7)
- [x] erledigt
- **Modell:** Opus · **Braucht:** E2.1, E2.4, E3.5, A.5, A.7 · **Dateien:** `crates/torroworkplace/src/{inbox,core,approvals,audit,client_setup}.rs`,
  `crates/torro-ipc/src/api.rs`, `docs/api.md`
- **Hoch (H1):** Die Quelle `suite` (Freigaben anderer Agenten, Kontoprobleme) sieht **kein** Client-Schlüssel,
  auch nicht bei `inbox_sources: All` oder ausdrücklicher Auswahl. Entschieden 2026-10-12 (Empfehlung, kein
  Widerspruch). Test: Client B sieht die Freigabe von Client A nicht (list/get/list_sources).
- **Mittel (M2):** Eine Freigabe für einen Inbox-Befehl merkt sich `updated` des Eintrags; hat er sich bis zur
  Ausführung geändert → `conflict`. `create_task_from_item` legt den Titel bei der Anfrage fest. Zusammenfassungen
  sagen, was läuft: `open_item` nennt Host, App oder „agent window“, `create_task_from_item` das Fälligkeitsdatum.
- **Mittel (M3):** `open_item` mit einem Web-Link braucht bei Client-Schlüsseln **immer** eine Freigabe
  (Zusammenfassung mit Host); Katalogbeschreibung passt sich an.
- **Mittel (M4):** `docs/api.md`: Inbox-Zeile mit `open_item` und `create_task_from_item` (inkl. Tasks-Prüfung),
  `clients.activity`, in „Was nie nach außen geht“ Punkt 1 die Ausnahme „Schlüssel eines neuen Clients einmal an
  die Steuerzentrale, die ihn in die Konfiguration des Agenten bzw. eine 0600-Schlüsseldatei schreibt“, Abschnitt
  zum Assistenten mit Gemini, Hooks, Schlüsseldateien, Sicherungen.
- **Niedrig:** `audit::ids`: Sammlungs-Kennungen nach dem Konto als Prüfsumme, Kennungen vom Client nur bei
  passendem Format (sonst Prüfsumme); abgelehnte Freigabe protokolliert die Kennungen der gespeicherten Anfrage;
  `client_setup::replace` verweigert kaputte Symlinks statt sie zu ersetzen.
- **API/MCP:** keine neuen Befehle.
- **Abnahme:** Tests je Punkt; `torro-api-review` ohne hohen Befund.

### E1.3 MCP, `torroworkplace notify` und Client für die App
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** E1.2, E3.1 · **Dateien:** `crates/torroworkplace/src/{mcp,cli}.rs`,
  `crates/torro-ipc/src/inbox.rs` (neu), `crates/torro-ipc/src/lib.rs` (nur `mod`), `crates/torroinbox/src/main.rs`
- **MCP:** Tools kommen aus dem Katalog; bequeme Eingaben: `inbox_list_items` ohne Angaben = für mich,
  offen, 50 Stück; `inbox_post_item` mit `title`, optional `body`, `url`. Antworten enthalten
  `untrusted` je Eintrag.
- **Kommandozeile:** `torroworkplace notify --title <t> [--body <b>] [--url <u>] [--for-me] [--thread <k>]`
  schickt `inbox.post_item` als `notice`, Schlüssel aus `TORRO_TOKEN` (Skript) oder der App-Schlüssel;
  mit dem App-Schlüssel ist die Quelle `script:cli`. Läuft der Dienst nicht, Fehlermeldung und
  Exit-Code ≠ 0 (anders als der Hook in E2.2).
- **Client:** `torro_ipc::inbox::InboxClient` setzt `InboxSource` über `core.sock` um (wie `PimClient`);
  `torroinbox` benutzt ihn ohne `--demo`.
- **Abnahme:** Vertragstest Tool ↔ Katalog; Tests, dass die bequemen Eingaben den richtigen Request
  bauen; `notify` ohne Dienst → Exit-Code 1; App zeigt ohne `--demo` die Einträge des Dienstes.

## E2 Suite und Agenten

### E2.1 Quelle „Suite“: Freigaben und Kontoprobleme
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** E1.2, A.3 · **Dateien:** `crates/torroworkplace/src/{inbox,approvals,pim}.rs`
- Jede offene Freigabe ist ein Eintrag (`approval`, dringend, Aktionen „Allow“/„Deny“, die die Oberfläche
  als `approvals.decide_approval` schickt; Name aus A.3). Entschieden oder abgelaufen → erledigt sich
  selbst. Die Zusammenfassung enthält Agententext und ist `untrusted`.
- Kontoprobleme (Passwort abgelehnt, neue Anmeldung nötig, Sync scheitert länger als 30 Minuten) sind
  Einträge `account_problem` mit Link in die Steuerzentrale; behoben → erledigt sich selbst.
- **API/MCP:** keine neuen Befehle; `approvals.decide_approval` bleibt **Never**.
- **Abnahme:** Tests: Freigabe anlegen → Eintrag, entscheiden → erledigt, Ablauf → erledigt;
  Kontoproblem kommt und geht; ein Agent kann beide nicht erledigen (`forbidden`).

### E2.2a Hook-Meldungen übersetzen (reine Logik)
- [x] erledigt
- **Modell:** Opus · **Braucht:** E1.1a · **Dateien:** `crates/torro-inbox/src/{lib,hook}.rs` (`hook.rs` neu),
  `crates/torro-inbox/tests/fixtures/hooks/*.json` (neu), `crates/torro-inbox/Cargo.toml` (falls `serde_json`
  als normale Abhängigkeit nötig)
- Liest das JSON eines Hooks (Claude Code, Codex, Gemini CLI) und macht daraus eine Absicht:
  „meldet `agent_waiting`/`agent_done` mit `AgentInfo`“, „löst das Warten dieser Sitzung auf“, „löst alles
  dieser Sitzung auf“ oder „nichts“. Zuordnung genau nach der Tabelle in E2.2; unbekannte Ereignisse und
  kaputtes JSON → „nichts“ (nie ein Fehler, der den Agenten stört).
- Projekt = letzter Teil von `cwd`; Branch aus dem Inhalt von `.git/HEAD`, den der Aufrufer übergibt
  (die Funktion liest keine Dateien). `thread_key` = `<agent>:<session_id>`. Meldung des Agenten als `body`.
- Fixtures: echte Beispiel-Payloads je Ereignis nach den offiziellen Hook-Dokumentationen (Claude Code,
  Codex, Gemini CLI), ohne persönliche Daten.
- **API/MCP:** keine; E2.2 schickt die Absichten als Requests.
- **Abnahme:** Fixture-Test je Zeile der Tabelle; unbekanntes Ereignis, fehlende Felder, kaputtes JSON →
  „nichts“; Branch aus `ref: refs/heads/x` und aus einer losgelösten Commit-Kennung.

### E2.2 `torroworkplace hook`: Agenten melden sich
- [x] erledigt
- **Modell:** Opus · **Braucht:** E1.3, E2.2a · **Dateien:** `crates/torroworkplace/src/hook.rs` (neu),
  `crates/torroworkplace/src/{cli,main}.rs`
- Nutzt die Übersetzung aus E2.2a; hier kommen stdin, `.git/HEAD` lesen, Elternprozesse, Schlüssel und
  Versand dazu.
- `torroworkplace hook claude-code|codex|gemini` liest das JSON des Hooks von stdin und schickt
  `inbox.post_item` bzw. `inbox.resolve_items` mit einem Hook-Schlüssel (`TORRO_TOKEN`):

  | Agent | Ereignis | Wird zu |
  |---|---|---|
  | Claude Code | `Notification` mit `permission_prompt`, `agent_needs_input`, `elicitation_dialog` | `agent_waiting` |
  | Claude Code | `Stop` | `agent_done` |
  | Claude Code | `UserPromptSubmit`, `PostToolUse` | erledigt das Warten dieser Sitzung |
  | Codex | `PermissionRequest` (nur beobachten, keine Entscheidung) / `Stop` / `UserPromptSubmit`, `PostToolUse` | wartet / fertig / erledigt |
  | Gemini CLI | `Notification` (`ToolPermission`) / `AfterAgent` / `BeforeAgent` | wartet / fertig / erledigt |
  | alle drei | `SessionEnd` | erledigt alles dieser Sitzung |

  `idle_prompt` wird ignoriert (`Stop` deckt es ab). `thread_key` = `<agent>:<session_id>` (der Kern
  stellt den Client davor). Projekt = letzter Teil von `cwd`; Branch aus `.git/HEAD` lesen (keinen
  Prozess starten). Die Meldung des Agenten geht als `body` mit (`untrusted`).
- **Darf den Agenten nie aufhalten oder ändern:** schreibt **nichts** auf stdout (Claude Code wertet
  stdout von Hooks aus), endet immer mit Exit-Code 0, höchstens 500 ms für Verbindung und Antwort;
  läuft der Dienst nicht, passiert nichts (kein Start des Dienstes).
- Unter Linux die Kette der Elternprozesse (`/proc/<pid>/stat`, höchstens 32 Stufen) als `agent.pids`
  mitschicken, für E2.4. Auf macOS und Windows leer (`#[cfg(target_os = "linux")]`).
- Bis E2.3 steht, legt man den Hook-Client von Hand an (`torroworkplace ctl`, App-Schlüssel):
  `torroworkplace ctl clients add "Claude Code hooks" --kind hook --agent claude-code` gibt den Schlüssel einmal aus; der
  Kern schneidet die Rechte auf `Rights::hook()` zu (nur `inbox` schreiben, keine Quellen sehen). Der
  Hook läuft dann z. B. als `TORRO_TOKEN=<Schlüssel> torroworkplace hook claude-code`.
  Umgesetzt: stdin höchstens 1 MiB (größer → nichts) mit höchstens 500 ms Warten, 500 ms für Verbindung und
  Antwort zusammen; stderr bleibt still, außer bei falschem Aufruf oder mit `TORRO_HOOK_DEBUG=1` (eine Zeile).
- **API/MCP:** nutzt E1.1.
- **Abnahme:** Fixture-Tests je Ereignis → richtiger Request; unbekanntes Ereignis → nichts; kein
  Byte auf stdout; ohne Dienst Exit-Code 0 in unter 500 ms.

### E2.3 Hooks beim Verbinden eines Agenten einrichten
- [x] erledigt (Gemini CLI kam dabei auch als MCP-Ziel in den Assistenten)
- **Modell:** Sonnet · **Braucht:** E2.2, A.7 · **Dateien:** `crates/torroworkplace/src/client_setup.rs`,
  `crates/torroworkplace/src/ui/agent_wizard.rs`, `crates/torroworkplace/tests/fixtures/client_setup/*`
- **Oberfläche:** Im Assistenten „Connect agent“ für Claude Code, Codex und Gemini CLI ein Häkchen
  „Report to Inbox when waiting or done“, vorausgewählt.
- Legt dafür einen eigenen Client der Art `hook` an („Claude Code hooks“; Sicherheitsregel 4: nur
  Inbox, keine Quellen sichtbar) und trägt die Hooks ein: Claude-Code-Einstellungen, `~/.codex/hooks.json`,
  Gemini-Einstellungen. Fremde Hooks bleiben, Schreiben atomar mit Sicherung, erneutes Einrichten ändert
  nichts doppelt. „Disconnect…“ entfernt Hooks und Hook-Client wieder.
- **API/MCP:** nutzt `clients.*` (App-Schlüssel, **Never**).
- **Abnahme:** Tests mit Konfigurationen unter `tests/fixtures/` (vorhandene fremde Hooks bleiben,
  zweimal einrichten = einmal, Entfernen sauber), kein echtes Home-Verzeichnis.

### E2.3b Hook-Schlüssel aus einer Datei statt aus der Befehlszeile
- [x] erledigt (Datei `<config>/hooks/<hook-client-id>.key`, Befehl `'<exe>' hook <agent> --key-file '<datei>'`;
  Eigentümer-Prüfung nur unter Linux über `/proc/self/status`, sonst nur die Rechte)
- **Modell:** Opus · **Braucht:** E2.3 · **Dateien:** `crates/torroworkplace/src/{hook,client_setup}.rs`,
  `crates/torroworkplace/tests/fixtures/client_setup/*`
- Heute steht der Hook-Schlüssel in der Hook-Befehlszeile (`TORRO_TOKEN='…' torroworkplace hook …`) und damit
  während jedes Aufrufs in `ps` sowie in der Agenten-Konfiguration. Stattdessen: Der Assistent legt den Schlüssel
  in eine eigene Datei (0600, im Konfigurationsverzeichnis des Kerns, je Hook-Client), der Hook-Befehl nennt nur
  die Datei (`torroworkplace hook claude-code --key-file <pfad>` oder `TORRO_TOKEN_FILE`), `hook.rs` liest sie
  (höchstens wenige hundert Bytes, Rechte prüfen: von anderen lesbar → nichts tun). „Trennen…“ löscht die Datei.
  Bestehende Einträge mit Schlüssel in der Befehlszeile werden beim erneuten Verbinden ersetzt.
- **API/MCP:** keine.
- **Abnahme:** Tests: Hook liest die Datei, zu offene Rechte → nichts, fehlende Datei → nichts (Exit 0, keine
  Ausgabe); Konfiguration enthält keinen Schlüssel mehr; Entfernen löscht die Datei.

### E2.4 Ins Fenster des Agenten springen
- [x] erledigt (`docs/api-reference.md` gibt es noch nicht, A.9)
- **Modell:** Opus · **Braucht:** E2.2 · **Dateien:** `crates/torroworkplace/src/{inbox,core,mcp}.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`, `docs/api-reference.md`
- Der Kern sucht zu `agent.pids` das Fenster über `torro-wm` (`windows()` mit `pid`) und setzt den
  Link `window`, nur bei Hook-Einträgen (Sicherheitsregel 6). Gesucht wird beim Öffnen, weil
  Fenster-Kennungen sich ändern.
- **API:** `inbox.open_item { id }` · Write – „öffnet den Eintrag: springt ins Fenster oder öffnet den
  Link“. Nur `http`/`https`, `app` aus `apps.list_apps`, `window` bei Hook-Einträgen. Fehler `not-found`,
  `unsupported` (kein Fenster, anderes Betriebssystem, unsicherer Link).
- **MCP:** `inbox_open_item` (Write; ein Agent kann damit den Blick des Menschen lenken, mehr nicht).
- Grenzen im ersten Schritt: Läuft der Agent in tmux, endet die Kette beim tmux-Server, dann gibt es
  keinen Sprung (Aktion ausgeblendet). macOS/Windows: Aktion ausgeblendet.
- Umgesetzt: Der Kern setzt `window` selbst, sobald ein Hook bei `agent_waiting`/`agent_done` Prozesse
  mitschickt; ist einer davon ein tmux-Server (`/proc/<pid>/comm` beginnt mit `tmux`), bleibt der Link
  weg, ebenso ohne Prozesse (macOS, Windows) – die App zeigt „Go to window“ dann nicht. Beim Öffnen
  gewinnt der nächste Prozess mit Fenster; hat er mehrere Fenster (ein Terminal, das alle Fenster aus
  einem Prozess bedient), ist die Antwort `unsupported`. Nach erfolgreichem Öffnen wird ein neuer
  Eintrag `seen`, wenn der Aufrufer ihn ändern darf (Regel 1). Die App schickt für jeden Link
  `inbox.open_item`; nur mit `--demo` öffnet sie Seiten und Apps weiter selbst.
- **Abnahme:** Tests mit `torro-wm`-Fake (Fenster mit pid): Treffer, Fenster weg → `unsupported`;
  `file:`-Link → `unsupported`; `window` an einem Nicht-Hook-Eintrag wird ignoriert.

### E2.5 Rückfragen von Agenten im Eingang beantworten
- [x] erledigt (Freigabe der Art `approvals.ask_permission`, Ablauf 32 s, höchstens drei offen je Client; Hook `--answer`, Timeout 40 s; offen: MCP-Ablauftext „ten minutes“ in `mcp.rs`, lange Rückfragen in der Benachrichtigung mit „Allow“, ob Claude Code seinen Dialog erst nach dem Hook zeigt)
- **Modell:** Opus · **Braucht:** E2.3b, E3.2, A.8b, E1.2c · Sicherheitsteil freigegeben 2026-10-12 ·
  **Dateien:** `crates/torroworkplace/src/{hook,approvals,core,inbox,notify,client_setup,audit}.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torro-inbox/src/hook.rs` (+ Fixtures),
  `crates/torroinbox/src/{app,detail}.rs` (nur falls die Freigabe-Einträge mehr brauchen), `docs/api.md`
- Claude Code und Codex fragen über den Hook `PermissionRequest` „darf ich?“. Der Hook legt eine
  Rückfrage an und wartet bis zu 30 s auf die Entscheidung des Menschen im Eingang, in der
  Steuerzentrale (Freigaben), mit `ctl allow|deny` oder in der Benachrichtigung („Allow“ / „Deny“).
  Ohne Entscheidung gibt der Hook **keine** Antwort, und der Agent fragt wie gewohnt im Terminal.
  Nie automatisch erlauben.
- **Bauweise (Empfehlung):** die Rückfrage ist eine **Freigabe** (A.3) mit eigener Art „Rückfrage eines
  Agenten“: Sie erscheint damit ohne neue Oberfläche in Inbox (E2.1), Tray, Steuerzentrale, `ctl approvals`
  und Benachrichtigung; „Erlauben“ führt nichts im Kern aus, sondern beantwortet nur die Rückfrage. Weicht die
  Umsetzung davon ab (eigene `inbox.ask_question`/`question_status`/`answer_question`), begründen.
- **API:** ein Befehl zum Fragen (Write, auch für Hook-Clients), einer zum Abholen der Antwort (Read, nur die
  eigene Rückfrage – Hook-Clients sehen sonst nichts, Regel 4), Entscheiden nur mit dem App-Schlüssel
  (`approvals.decide_approval` bzw. `Mcp::Never`, Punkt 3 in api.md).
- **Hook:** `torroworkplace hook claude-code|codex --answer` (o. ä.) für das Ereignis `PermissionRequest`;
  antwortet im Format des jeweiligen Agenten (offizielle Doku prüfen: Claude Code
  `hookSpecificOutput.decision.behavior` allow/deny, Codex entsprechend); gibt sonst nichts aus, Exit 0.
  E2.3 richtet den Hook mit passend langem Timeout (> 30 s) ein; bestehende Einrichtungen werden beim erneuten
  Verbinden ergänzt.
- **Sicherheit:** genauer Wortlaut des Werkzeugs und Befehls ungekürzt sichtbar (aber als `untrusted`, ohne
  Steuer-/Bidi-Zeichen), Entscheidung an genau diese Rückfrage gebunden (Kennung, Ablauf 30 s + Spielraum),
  Antwort nur über den App-Schlüssel, Hook nimmt nur eine Antwort zur eigenen Rückfrage an; Protokoll ohne Inhalt.
- **Abnahme:** Tests: Zeitablauf → keine Ausgabe; „Deny“ → Ablehnung im Format des Agenten; „Allow“ →
  Erlaubnis im Format des Agenten; Client-Schlüssel kann nicht antworten; Hook-Client sieht fremde Rückfragen
  nicht; `torro-api-review` ohne hohen Befund.

## E3 App `torroinbox`

### E3.1 App-Gerüst
- [x] erledigt (Befunde der Design-Prüfung in E3.2b)
- **Modell:** Opus · **Braucht:** E1.1a · **Dateien:** `crates/torroinbox/` (neu), Wurzel-`Cargo.toml` (Mitglied)
- Iced-App mit `torro_gui::shell`, Fensterklasse `torro-inbox`. Seitenleiste: **For me** (Zahl),
  **Info**, **Snoozed**, **Done**; Gruppe **Sources** aus `inbox.list_sources` (Suite, Agents, Scripts).
  Liste: Quellen-Symbol, Titel, Vorschau, Zeit, Zähler. Paneel (Muster U.3): bei Agenten Projekt,
  Branch, Meldung und Aktionen; bei Freigaben „Allow“/„Deny“.
- Größen Breit, Mittel, Schmal; Mini und Leiste folgen in E3.3.
- Arbeitet gegen `InboxSource` aus E1.1a; `--demo` nimmt `DemoSource`, `--demo --open <state>` für die
  Sichtprüfung. Ohne `--demo` vorerst eine leere Quelle mit leerem Zustand; den echten Client bindet E1.3 an.
- Leerer Zustand: „Nothing for you right now.“
- **API/MCP:** kommt über E1.1/E1.3; die App schickt nur, was `InboxSource` anbietet.
- **Abnahme:** `torro-design-review` in allen Größen, hell und dunkel; Tests der Nachrichten.

### E3.2 Abarbeiten mit der Tastatur
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** E3.1 · **Dateien:** `crates/torroinbox/src/{app,keys}.rs`
- `j`/`k` bewegen, `Enter` öffnen, `e` erledigt, `u` rückgängig (Hinweis 5 s), `s` vertagen (Later today,
  Tomorrow, Next week, Pick…), `m` Quelle stumm (mit Nachfrage), `l` Spur wechseln (fragt „Always for
  this source?“). Alles auch mit der Maus.
- `Enter` öffnet bis E2.4 nur `http`/`https`-Links im Browser; mit E2.4 geht es über `inbox.open_item`.
- Läuft mit der Demo-Quelle, bevor der Kern steht.
- **API/MCP:** nutzt `inbox.set_item_done`, `snooze_item`, `set_item_lane`, `mute_source`.
- **Abnahme:** Tests je Taste, Rückgängig.

### E3.2b Feinschliff nach der Design-Prüfung von E3.1
- [x] erledigt (zweite Prüfung: alte Befunde behoben, neue in E3.2c)
- **Modell:** Opus · **Braucht:** E3.2 · **Dateien:** `crates/torroinbox/src/{app,list,detail}.rs`
- Aus der Prüfung am 2026-10-11 (Bilder: Scratchpad `review-inbox/`):
  - **Muss:** Kein hart abgeschnittener Text. Vorschau nur, wenn neben dem Titel genug Platz ist
    (etwa 120 px), sonst weglassen; in Schmal zweizeilige Zeilen (Titel oben, Vorschau darunter in META).
  - **Muss:** Dringendes nicht nur über die Farbe. „For me“ gruppiert wie torrotasks
    („Waiting for you“ / „Earlier“, Gruppenkopf) oder ein Status-Punkt mit Wort vor der Zeit.
  - **Muss:** Fehlertext als ganzer Satz, z. B. Titel „Couldn't load the inbox.“ und darunter leise der Grund.
  - **Soll:** Liste in Breit links bündig wie die Schwester-Apps (mit offenem Paneel Breite `LIST`), nicht
    mittig; in der Leiste (Rail) die Quellen weglassen oder unterscheidbar machen; Zahlen der Quellen nur
    für die gezeigte Spur oder als „offen insgesamt“ erkennbar; im Kopf nur die Zahl statt „6 for you“;
    im Freigabe-Paneel den Hinweis zu Allow/Deny direkt über die Knöpfe; leerer Zustand mit Symbol.
  - **Schön:** „until Wed, 14 Oct“ bei Vertagtem; im Paneel zusätzlich die relative Zeit.
- **API/MCP:** keine.
- **Abnahme:** `torro-design-review` ohne Muss-Befund; Bilder mit Fenster ganz auf dem Bildschirm
  (die weißen Streifen der ersten Prüfung kamen vom Fenster über dem Bildschirmrand).

### E3.2c Zweite Design-Prüfung: Nacharbeit
- [x] erledigt
- **Modell:** Opus · **Braucht:** E3.2b · **Dateien:** `crates/torroinbox/src/{app,list,detail}.rs`,
  `crates/torro-gui/src/theme.rs` (nur ein neues Token `size::ROW_TWO`), `crates/torro-gui/src/panel.rs`
  (nur der Durchschein-Fehler)
- Aus der Prüfung am 2026-10-12 (Bilder: Scratchpad `review-e32b/`):
  - **Muss:** Zweizeilige Zeilen (Schmal, Mini) bekommen eigene Höhe `size::ROW_TWO` (52), `row_span`
    rechnet damit; die Zeit steht auf der Zeile des Titels.
  - **Muss:** In Schmal scheint der Auswahlbalken der Liste links durch das darübergelegte Paneel (und bei
    Skalierung 1,6 eine Pixelspalte bei x=0). Das Paneel deckt dort die ganze Breite deckend ab.
  - **Muss:** Stummschalten ohne Dialog und ohne roten Knopf: sofort stumm, mit dem Hinweis „Muted ·
    Undo“ wie bei den anderen Änderungen (Regel: bestätigen *oder* rückgängig, nicht beides).
  - **Soll:** Breit mit offenem Paneel: Liste füllt bis `CONTENT_MAX` statt `LIST` (keine 490 px Lücke).
  - **Soll:** Vertagen-Menü zeigt die Zeit („13:30“, „Wed 09:00“) leise vor dem Tastenhinweis; die Auswahl
    nennt im Titel das volle Datum, nicht zweimal den Tag, Knopf rechts wie im Dialog.
  - **Soll:** Zahl im Kopf (Mittel) direkt am Menüknopf; Spur-Knopf im Paneel mit Wort („Move to Info“)
    in Breit und Mittel; Fehlerzeile mit Symbol in Fehlerfarbe und Text in TEXT_2.
  - **Schön:** Befehle in der Meldung nicht am „--“ umbrechen; Symbol im leeren Zustand größer (Token).
- **API/MCP:** keine.
- **Abnahme:** `torro-design-review` ohne Muss-Befund.

### E3.3a Mini und Leiste
- [x] erledigt (offen aus der Prüfung: Ladefehler in Mini/Leiste zeigt noch die große Fehleransicht statt einer ruhigen Kachel)
- **Modell:** Opus · **Braucht:** E3.2c · **Dateien:** `crates/torroinbox/src/glance.rs` (neu),
  `crates/torroinbox/src/{app,main}.rs` (nur `mod` und Umschalten der Ansicht)
- Mini/Leiste sind Anzeigen, keine Mini-Apps (Design-Regel): „3 for you“ und die drei obersten Einträge
  mit Quellen-Symbol; Dringendes hervorgehoben (nicht in Torro-Rot); leer: ruhiger Satz. Klick öffnet die
  App groß bzw. den Eintrag (wie die Glance-Ansichten von torrotasks und torrocalendar).
- **API/MCP:** keine; liest über `InboxSource`.
- **Abnahme:** `torro-design-review` für Mini und Leiste, hell und dunkel; Tests der Nachrichten.

### E3.3b Tray-Zahl
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** E1.2, A.8 (gleiche Tray-Datei) · **Dateien:**
  `crates/torroworkplace/src/tray/linux.rs`
- Tray: **eine** Zahl für offene Einträge „für mich“ (Freigaben sind jetzt Einträge, die Zahl aus A.8
  geht darin auf), Untermenü mit den fünf neuesten, „Open Inbox“.
- **Abnahme:** Test der Tray-Zahl.

### E3.4 Desktop-Benachrichtigungen und Ruhe
- [x] erledigt (Popups in `inbox_notify.rs`; Freigaben fragen weiter nur über `approvals.rs`; Klick öffnet `torroinbox`, zum Eintrag springen fehlt noch ein Kanal zur App; `Event::transparency` aus `TRANSP`; Einstellung `quiet_when_busy` über `settings.set_setting`)
- **Modell:** Opus · **Braucht:** E1.2, E2.1 (Feld `TRANSP` freigegeben 2026-10-12) · **Dateien:** `crates/torroworkplace/src/{notify,inbox}.rs`,
  `crates/torro-core/src/settings.rs`, bei Punkt 2 zusätzlich `crates/torro-pim/src/*` und
  `crates/torro-pim-caldav/src/ical.rs` (nennen)
- Popups nur für „für mich“, Dringendes mit Ton. Knöpfe: „Done“, „Snooze 1 h“, Freigaben „Allow“/„Deny“;
  Klick öffnet `torroinbox` beim Eintrag. Mehr als drei in einer Minute → eine Meldung „5 new for you“.
- **Ruhe:** Einstellung „Quiet during busy events“ (Standard an; Schalter auf der Inbox-Seite, E3.7).
  Läuft ein belegter Termin (nicht ganztägig, nicht „frei“), kommen nur dringende Popups; danach
  **eine** Sammelmeldung.
- **API/MCP:** Einstellung über die vorhandenen `settings.*`-Befehle (neues optionales Feld, Admin).
- **Abnahme:** Tests mit fester Uhr und Fake-Benachrichtiger: Info → kein Popup; im belegten Termin nur
  dringend; Sammelmeldung danach; Bündeln ab vier.

### E3.5 Zur Aufgabe machen
- [x] erledigt (`docs/api-reference.md` gibt es noch nicht, A.9; Rückweg: `link.id = inbox:<id>`, Webseite als `url`, Text als Beschreibung; Freigabe über das `confirm` des Clients wie bei jedem Write)
- **Modell:** Opus · **Braucht:** E3.2, E1.2, P.2 · **Dateien:** `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torroworkplace/src/{core,inbox,mcp}.rs`, `crates/torroinbox/src/app.rs`, `docs/api-reference.md`
- **Oberfläche:** Taste `t` öffnet einen kleinen Dialog (Titel vorbelegt, Liste, Fälligkeit). Die
  Aufgabe bekommt den Link des Eintrags (Feld aus P.2), der Eintrag ist danach erledigt.
- **API:** `inbox.create_task_from_item { id, collection, title?, due? }` · Write (Bereich `inbox`).
  Weil eine Operation nur einen Bereich kennt, prüft der Handler zusätzlich selbst: Tasks-Schreibrecht,
  Listen-Auswahl (ausgeblendete oder nicht freigegebene Liste → `not-found`, nicht `forbidden`) und
  „Freigabe vorher“ von Tasks; A.3 prüft beim Ausführen einer Freigabe alles erneut.
- **MCP:** `inbox_create_task_from_item`.
- **Abnahme:** Tests: Aufgabe entsteht mit Link, Eintrag erledigt; ohne Tasks-Recht → `forbidden`;
  ausgeblendete Liste → `not-found`.

### E3.6 In die Suite aufnehmen
- [x] erledigt
- **Modell:** Haiku · **Braucht:** E3.1 · **Dateien:** `crates/torroworkplace/src/apps.rs`,
  `crates/torroinbox/src/main.rs`, `docs/app-catalog.md`
- Startbar aus Tray und Steuerzentrale (Apps), Fensterklasse `torro-inbox`, Argumente `--lane` und
  `--source` für Fenster je Ausschnitt, gemerkt in Workspace-Vorlagen. Im App-Katalog „Inbox“ eintragen;
  „Benachrichtigungen“ und „Freigaben-Eingang“ als darin aufgegangen markieren.
- **Abnahme:** App startet aus dem Tray; Vorlage mit `--lane info` stellt das Fenster wieder her.

### E3.7 Steuerzentrale: Seite „Inbox“
- [x] erledigt (zusätzlich `ui/launch.rs` für Demo-Regeln und `--open inbox|inbox-empty`; „Remove“ ohne Rückfrage, weil nichts verloren geht und die Regel in der Inbox mit einem Griff wiederkommt)
- **Modell:** Sonnet · **Braucht:** E1.2, E3.4 · **Dateien:** `crates/torroworkplace/src/ui/inbox.rs` (neu),
  `crates/torroworkplace/src/ui/mod.rs`
- **Oberfläche:** Gruppe „Notifications“ mit „Quiet during busy events“; Gruppe „Rules“ mit gemerkten
  Spur-Regeln und stummen Quellen, je mit „Remove“.
- **API/MCP:** nutzt `inbox.list_rules`, `inbox.remove_rule`, `settings.*`.
- **Abnahme:** Sichtprüfung (Breit, Schmal, hell/dunkel); Tests der Nachrichten.

### E3.8 Freigaben in der Inbox entscheiden, Rückfragen richtig zeigen
- [x] erledigt (Sichtprüfung `--demo --open approval` in 1300×800 und 480×800, hell und dunkel; API-Prüfung
  freigegeben, ihre Doku-Hinweise sind eingearbeitet)
- **Modell:** Opus · **Braucht:** E2.1, E2.5, E3.1 · **Dateien:** `crates/torro-inbox/src/{source,demo,hook}.rs`,
  `crates/torro-inbox/tests/fixtures/hooks/` (neue Nutzlast), `crates/torro-ipc/src/inbox.rs`,
  `crates/torroinbox/src/{app,detail,empty,list}.rs`, `crates/torroworkplace/src/{inbox,hook}.rs` (nur der Titel),
  `crates/torro-core/src/i18n/de.rs`; Doku: `docs/{api,eingang}.md`, Doc-Kommentar in
  `crates/torroworkplace/src/client_setup.rs` (aus der API-Prüfung)
- Rückmeldung 2026-10-12: In der Inbox stehen „Allow“ und „Deny“ ausgegraut mit „not available here yet“.
  Der Eintrag war ein `AskUserQuestion` von Claude Code, als Freigabe mit rohem JSON.
- **Entscheiden:** `InboxSource::decide_approval(approval, allow)` schickt `approvals.decide_approval` mit dem
  App-Schlüssel (die App ist der Mensch, Sicherheitsregel 1). Die Knöpfe sind aktiv, solange die Entscheidung nicht
  unterwegs ist; danach Hinweis „Allowed“/„Denied“ und neu laden. Schon entschieden oder abgelaufen (`conflict`):
  Hinweis „This was decided already or has expired.“ Keine Taste dafür: ein verirrter Tastendruck soll nichts
  erlauben.
- **Rückfragen ohne Erlaubnis:** `AskUserQuestion` ist keine Freigabe, sondern eine Frage im Terminal. Der Hook
  stellt sie nicht als `approvals.ask_permission`, sondern meldet „wartet“ mit den Fragen und Antworten als Text.
  Der Titel von `agent_waiting` heißt allgemein „{agent} is waiting for you“ (auch für Eingabe und Auswahl).
- **Abgelaufen heißt erledigt – gibt es schon:** Der Kern erledigt den Eintrag einer Rückfrage beim Ablauf
  (E2.5, `file_when_expired`; im Fall der Rückmeldung 24 ms danach). „1 min ago“ heißt „vor weniger als zwei
  Minuten“ (`ago` zeigt mindestens 1): Das Bild entstand noch innerhalb der 32 s. Nichts zu ändern.
- **Oberfläche:** Inbox, Paneel. **API:** `approvals.decide_approval` (gibt es, Admin, nur App-Schlüssel).
  **MCP:** Never (Freigaben entscheidet nur der Mensch, `api.md`).
- **Abnahme:** Tests: Entscheiden über Demo- und Socket-Quelle, `conflict` wird zum eigenen Fehler; App-Nachrichten
  (Knöpfe gesperrt während der Entscheidung, Hinweise); `AskUserQuestion` wird keine Frage, sondern „wartet“ mit
  Text. Sichtprüfung des Paneels mit `--demo --open approval`.

## Schaltzentrale (Issue #71, 2026-10-13)

Rückmeldung: Der Eingang ist zu dicht, Mails und Benachrichtigungen sind gemischt, die wichtigen Aktionen
sitzen nur hinter Tasten. Entwürfe (Varianten A, B, C und Vorbilder):
https://claude.ai/artifact/HKD7No5s6e8jc4y4ounrDs. **Entschieden:** Variante A mit den Info-Bündeln aus C;
Aktionen beim Überfahren und am Tastaturcursor; „Task“ öffnet den vorhandenen Dialog aus E3.5.
Reihenfolge **E2.6 → E3.10 → E3.9**, nie parallel (gleiche Dateien in `torroinbox`, E3.10 ändert den Katalog).
**Erst nach L.2** ([design-pakete.md](design-pakete.md)), weil L.2 jede App umstellt; E3.9 nach L.5, damit die neuen
Zeilen gleich mit austauschbaren Werten entstehen.

### E2.6 Sitzungsname in Meldungen von Agenten
- [x] erledigt (Freigabe-Frage: `AskPermission.title`, nur von Hook-Clients, MCP-Tool ohne das Feld – mit Issue #71
  freigegeben, dafür auch `torro-ipc/src/{lib,api}.rs`, `torroworkplace/src/{core,mcp}.rs`, `docs/api.md`;
  Mini/Leiste in E3.9; Sichtprüfung `--demo --open detail` hell und dunkel auf macOS, nur breit)
- **Modell:** Opus · **Braucht:** E2.2, E2.5, E3.8, L.2 · **Dateien:** `crates/torro-inbox/src/{item,hook,demo}.rs`,
  `crates/torro-inbox/tests/fixtures/hooks/` (neue Transkript- und Index-Beispiele),
  `crates/torroworkplace/src/{hook,inbox,inbox_notify,approvals}.rs`, `crates/torroinbox/src/{list,detail}.rs`,
  `crates/torro-core/src/i18n/de.rs`; Doku: `docs/eingang.md` (Abschnitt Agenten)
- Die Desktop-App von Claude Code zeigt jede Sitzung unter dem Projekt mit Namen. Diesen Namen zeigt der Eingang
  bei „wartet“, „fertig“ und bei Rückfragen, damit man beim Freigeben weiß, um welchen Job es geht.
- **Woher:** Claude Code: im Transkript (`transcript_path` der Nutzlast) die letzte Zeile
  `{"type":"custom-title","customTitle":"…"}`, ohne sie die letzte `{"type":"ai-title","aiTitle":"…"}`. Claude Code
  schreibt die Zeile regelmäßig neu, es reicht das Ende der Datei (höchstens 256 KB; nichts gefunden → kein Name).
  Codex: `$CODEX_HOME/session_index.jsonl` (Standard `~/.codex`), letzte Zeile mit `"id" == session_id`, Feld
  `thread_name`. Gemini CLI: kein Name.
- **Robust:** Interne, undokumentierte Formate. Fehlt etwas oder passt das Format nicht, bleibt alles wie heute; der
  Hook scheitert daran nie und wird nicht merklich langsamer. Gelesen wird nur bei Meldungen (`Report`) und
  Rückfragen, nicht bei `PostToolUse`. Aus dem Transkript wird nur der Titel genommen, sonst nichts.
- **Fremder Text:** Steuer- und Richtungszeichen entfernen, Leerraum zusammenfassen, auf 80 Zeichen kürzen. Der
  Eintrag bleibt `untrusted`; der Titel wird nie Teil eines `tr()`-Schlüssels.
- **Schnittstelle (freigegeben mit Issue #71):** `AgentInfo.title: Option<String>` und
  `PermissionQuestion.title: Option<String>`, beide `#[serde(default)]`. `hook::interpret` bleibt rein; der Aufrufer
  liest den Titel (wie `.git/HEAD`) und reicht ihn hinein. Ein neuer Bericht derselben Sitzung übernimmt einen
  geänderten Namen (`rules::merge`).
- **Oberfläche:** Zeile: Sitzungsname als Titel, darunter „{agent} is waiting for you · Projekt · Branch“; ohne Namen
  wie heute. Paneel: Name als Überschrift. Desktop-Benachrichtigung und Freigabe-Frage nennen den Namen.
  **API:** vorhandene `inbox.post_item`, `inbox.list_items`, `inbox.get_item` (Feld kommt dazu). **MCP:** vorhandene
  `inbox_list_items`, `inbox_get_item`; Schema-Beschreibung nennt das Feld.
- **Abnahme:** Tests mit Beispieldateien: letzter `custom-title` gewinnt, `ai-title` als Ersatz, großes Transkript
  (Titel nur am Ende), kaputte Zeilen, fehlende Datei, Codex-Index, Gemini ohne Namen, Steuerzeichen und Länge.
  Demo-Daten mit Sitzungsnamen. `torro-api-review`.

### E3.10 Ausblenden in Stufen
- [x] erledigt (zusätzlich nur Match-Arme bzw. Literale in `torroworkplace/src/{approvals,audit,daemon,pim}.rs`,
  `torroinbox/src/empty.rs`, `torroworkplace/src/ui/{launch,mod}.rs` – zwei Demo-Regeln für die Sichtprüfung; „…“ im
  Paneel ersetzt den Mond-Knopf, daneben zeigt „Move to …“ nur sein Icon; „This conversation“ ist `set_item_done`
  und markiert die Mail darum gelesen – Rückfrage offen; Sichtprüfung `--demo --open hide|hide-panel` und
  `torroworkplace ui --demo --open inbox` auf macOS, hell und dunkel)
- **Modell:** Opus · **Braucht:** E2.6, E3.7, E4.1 · **Dateien:** `crates/torro-inbox/src/{item,rules,source,demo}.rs`,
  `crates/torro-ipc/src/{lib,api,inbox}.rs`, `crates/torroworkplace/src/{core,inbox,mcp}.rs`,
  `crates/torroworkplace/src/ui/inbox.rs`, `crates/torroinbox/src/{app,keys,detail}.rs`,
  `crates/torro-core/src/i18n/de.rs`; Doku: `docs/{api,eingang}.md`
- „Hide“ ist kein Spam: Die Mail bleibt **unverändert** im Postfach (auch nicht als gelesen markiert), sie erscheint
  nur nicht mehr im Eingang. So stellt der Mensch nach und nach ein, was die Übersicht zeigt.
- **Menü „Hide from inbox“** (Taste `m`, im „…“-Menü des Paneels, in E3.9 auch in der Aktionsleiste):
  - Mail: „This conversation“ (erledigt; neue Antworten kommen wieder), „Everything from <Adresse>“,
    „Everything from <Domain>“, Trenner, „Always to Info“ (gibt es: `set_item_lane` mit `remember`),
    „Move to junk“ (gibt es: `mail.move_messages` in den Ordner mit Rolle Junk; fehlt er, fehlt der Eintrag).
  - Andere Quellen: „This entry“ (erledigt), „Mute <Quelle>“ (gibt es: `mute_source`); Suite nie.
- **Wirkung:** Neue Mail des Absenders bzw. der Domain wird kein Eintrag mehr; offene Einträge, die passen, werden
  erledigt, ohne in die Quelle zurückzuwirken. Domain heißt der Teil nach `@`, klein, genau (keine Subdomains).
- **Schnittstelle (freigegeben mit Issue #71):** `Rule.domain: Option<String>` (`#[serde(default)]`, neben
  `sender`); `RuleEffect::Muted` gilt dann nur für diesen Absender bzw. diese Domain.
- **API:** neu `inbox.hide_sender { id, scope: sender | domain }` · Admin (dauerhafte Regel wie `mute_source`);
  `not-found` für fremde oder unbekannte Einträge, `error` für Einträge ohne Mail-Absender. Antwort: die neue `Rule`.
  Katalogeintrag, `docs/api.md`. **MCP:** `inbox_hide_sender` (Admin, wartet auf Freigabe).
- **Steuerzentrale › Inbox › Rules:** Absender- und Domain-Regeln mit Wort („Hidden: news@shop.example“,
  „Hidden: everything from shop.example“) und „Remove“.
- **Abnahme:** Tests: Regel für Absender und Domain, neue Mail wird kein Eintrag, offene werden erledigt ohne
  Rückwirkung, Großschreibung egal, Subdomain nicht erfasst, Rechte (Write reicht nicht), `remove_rule` hebt auf.
  `torro-api-review`; Sichtprüfung des Menüs.

### E3.9 Übersicht in zwei Spalten mit mehr Luft
- [x] erledigt (zusätzlich `torroinbox/src/main.rs` (`mod overview`), Tokens auch in
  `torro-core/src/design{.rs,/rules.rs}` und `docs/design-pakete.md`; `l` wechselt in der Übersicht die Spalte statt
  der Spur; Demo-Zustände `overview`, `overview-info`, `overview-notices`; offen: „Info“ aufgeklappt nur im Speicher,
  nicht in `ui.json` (braucht `torro-gui/src/ui_state.rs`), Ordner-Ansichten behalten ihre Zeilen (braucht
  `mail.rs`); Menü „Hide from inbox“ so breit wie nötig, höchstens `size::panel()`; Sichtprüfung auf macOS
  (AeroSpace, eigenes Fenster schwebend) in allen sechs Größen, hell, dunkel und mit `plex`)
- **Modell:** Opus · **Braucht:** E2.6, E3.10, L.5 · **Dateien:** `crates/torroinbox/src/{app,list,keys,detail,glance}.rs`,
  `crates/torroinbox/src/overview.rs` (neu), `crates/torro-gui/src/{theme,widgets}.rs` (Tokens, Baustein
  Aktionsleiste), `crates/torro-gui/examples/gallery.rs`, `.claude/skills/torro-design/SKILL.md` (neue Tokens und
  Baustein dokumentieren), `crates/torro-core/src/i18n/de.rs`
- **Ansicht „Overview“** (neu, oberster Eintrag der Seitenleiste, Standard beim Start; Zahl = offene „For me“):
  zwei Spalten **Mail** (Einträge der Quelle `mail`) und **Notifications** (alle anderen), je mit Kopf 44 (Icon,
  Name, Zahl). In jeder Spalte zuerst „For me“, gruppiert: „Waiting for you“ (nur Benachrichtigungen), „Today“,
  „Yesterday“, „Earlier“; am Ende eingeklappt „Info · n“ (ein Klick klappt auf, gemerkt in `ui.json`). Die anderen
  Ansichten (For me, Info, Snoozed, Done, Quellen, Ordner) bleiben einspaltig, bekommen aber dieselben Zeilen.
- **Mail-Zeile** (Token `size::ROW_MESSAGE`, etwa 88): Avatar 32 rund mit Initialen (Fläche getönt in der Farbe des
  Kontos bzw. neutral), Zeile 1 Absender `BODY_STRONG`, Domain `META`/`TEXT_3`, rechts Zeit tabellarisch; Zeile 2
  Betreff (`BODY_STRONG` solange neu, sonst `BODY`); Zeile 3 Vorschau `TEXT_2` einzeilig mit „…“; rechts in Zeile 2
  bzw. 3 Konto als Punkt mit Name und Zahl der Nachrichten ab 2. Neu = kleiner Punkt in `TEXT` am Avatar, nie Rot.
  Gelesenes, aber offenes wird nicht blasser als `TEXT_2`.
- **Benachrichtigungs-Zeile** (Token `size::ROW_NOTICE`, etwa 64): Icon 28 der Quelle (Status wie E3.2b nur im Icon),
  Zeile 1 Titel, Zeile 2 Text bzw. „Projekt · Branch“, rechts Zeit. **Wartet** (Agent wartet, Freigabe): kleine Karte
  (`RAISED`, Rand `LINE_STRONG`, Radius `MD`, Innenrand `M`) mit den Knöpfen des Paneels (Allow/Deny aus E3.8,
  „Go to window“ aus E2.4), Sitzungsname aus E2.6 als Titel.
- **Aktionsleiste** (neuer Baustein `widgets::row_actions`): schwebt rechts in der Zeile beim Überfahren und an der
  Zeile unter dem Tastaturcursor; Fläche `MENU`, Rand `LINE_STRONG`, Radius `SM`, Knöpfe `CONTROL_SM` mit Icon und
  Wort: Task (`t`, Dialog aus E3.5), Done (`e`), Later (`s`, Menü wie heute), Hide (`m`, Menü aus E3.10). Ist die
  Spalte schmaler als die Leiste mit Wörtern, nur Icons mit Tooltip. Freigaben und Suite-Einträge zeigen nur, was für
  sie geht (keine Aufgabe aus einer Freigabe, Suite nie stumm), wie im Paneel.
- **Tastatur:** `j/k` innerhalb der Spalte, `h/l` bzw. ←/→ wechselt die Spalte, `Tab` wie bisher; `Enter` öffnet.
- **Größen:** Breit: zwei Spalten, je höchstens `CONTENT_MAX`, `LINE` dazwischen; offenes Detail (`PANEL` 400)
  ersetzt die Spalte, in der der Eintrag nicht steht. Mittel und Schmal: Umschalter `segmented` „Mail | Notifications“
  in der Kopfleiste, eine Spalte. Mini und Leiste unverändert. Leer je Spalte eigener ruhiger Leerzustand.
- **Oberfläche/API/MCP:** nur Oberfläche; nutzt vorhandene Requests (`inbox.list_items`, `set_item_done`,
  `snooze_item`, `create_task_from_item`, `hide_sender`, `mute_source`, `approvals.decide_approval`). Kein neuer Request.
- **Sitzungsname auch in Mini und Leiste** (aus E2.6): `glance.rs` zeigt bei Agenten mit Namen den Namen
  (`list::session_name`) statt „{agent} is waiting for you“, wie Zeile und Paneel.
- **Abnahme:** Tests: Aufteilung in Spalten und Gruppen, Info eingeklappt, Cursor zwischen Spalten, Aktionen je Art,
  Größenklassen. `scripts/design-check.sh` sauber. Sichtprüfung `torro-design-review` in allen Größen, hell und dunkel
  (`--demo --open overview`).

### E3.11 Nacharbeit: ungelesen ausblenden, Regeln für alle Konten, Undo
- [x] erledigt (zusätzlich nur `torroinbox/src/empty.rs` (Trait-Methode `remove_rule`) und ein Audit-Text in
  `torroworkplace/src/daemon.rs`; bestehende Regeln je Konto werden als „alle Konten“ gelesen, gespeichert beim
  Konto ihrer Entstehung (bestimmt, wer sie sieht); ein Schlüssel ohne Zugriff auf jenes Konto bekommt eine eigene
  Regel statt der fremden; Undo nach Done öffnet nur den Eintrag, die Mail bleibt gelesen; vorher vertagte Einträge
  kommen nach Undo offen zurück; Ordner-Zeilen ohne Aktionsleiste; Sichtprüfung auf macOS hell/dunkel, Breit/Schmal)
- **Modell:** Opus · **Braucht:** E3.9, E3.10 · **Dateien:** `crates/torro-inbox/src/{item,rules,source,demo}.rs`,
  `crates/torro-ipc/src/{lib,api,inbox}.rs`, `crates/torroworkplace/src/{core,inbox,mcp,approvals}.rs`,
  `crates/torroworkplace/src/ui/inbox.rs`, `crates/torroinbox/src/{app,list,overview,mail,detail,keys}.rs`,
  `crates/torro-gui/src/{widgets,ui_state}.rs`, `crates/torro-gui/examples/gallery.rs`,
  `.claude/skills/torro-design/SKILL.md`, `crates/torro-core/src/i18n/de.rs`; Doku: `docs/{api,eingang}.md`
- **Entschieden 2026-10-13 (Nutzer):**
  1. **„This conversation“ lässt die Mail ungelesen.** Ausblenden fasst das Postfach nie an. Schnittstelle (freigegeben):
     `HideScope::Conversation` in `inbox.hide_sender`; Zugriff je Umfang: `conversation` Write (keine dauerhafte
     Regel), `sender`/`domain` Admin. Neue Antworten im Gespräch kommen wieder.
  2. **Absender- und Domain-Regeln gelten für alle Mail-Konten.** Eine Regel blendet in jedem Konto aus. Bestehende
     Regeln je Konto gelten weiter (beim Laden zusammenführen oder als „alle Konten“ lesen; im Bericht begründen).
     Steuerzentrale › Inbox › Rules zeigt „All mail accounts“ statt des Kontos.
  3. **Undo nach Hide, Done und Later:** neuer Baustein `widgets::toast` (unten mittig, Fläche `MENU`, Rand
     `LINE_STRONG`, Radius `MD`, Schatten, Text `BODY` + Knopf „Undo“, verschwindet nach 5 s oder mit `Escape`;
     `Ctrl+Z` löst Undo aus). Undo nimmt genau die Wirkung zurück: Done → wieder offen; Later → nicht mehr vertagt;
     Hide Gespräch → wieder offen; Hide Absender/Domain → Regel entfernen und die dadurch erledigten Einträge wieder
     öffnen (dafür nennt die Antwort von `hide_sender` die erledigten Ids; Schnittstelle freigegeben). In Skill und
     Galerie dokumentieren.
- **Nacharbeit aus E3.9:**
  - Aufgeklapptes „Info“ je Spalte in `ui.json` merken (Helfer in `torro_gui::ui_state`).
  - Ordner-Ansichten (`mail.rs`) bekommen dieselben Mail-Zeilen wie die Übersicht.
  - Demo: Mails und Hinweise auch von gestern und früher, damit alle Gruppen im Bild sind.
  - Die Aktionsleiste sitzt auf der Höhe der ersten Zeile (Zeit) und deckt weder Vorschau noch Konto ab.
- **API/MCP:** `inbox.hide_sender` (erweitert), `inbox_hide_sender` (Beschreibung: Umfang und Rechte je Umfang);
  Undo nutzt vorhandene Requests (`set_item_done`, `snooze_item`, `remove_rule`).
- **Abnahme:** Tests: Gespräch ausblenden lässt Mail ungelesen (Fake-IMAP, kein STORE), Write reicht für
  `conversation`, nicht für `sender`; Regel wirkt in zwei Konten; Undo je Aktion; `ui.json` merkt „Info“.
  `torro-api-review`; Sichtprüfung (Toast, Aktionsleiste, Ordner-Ansicht) hell und dunkel.

## E8 Drei Fenster (2026-10-13)

Konzept: [eingang.md › Drei Fenster](../eingang.md#drei-fenster-entschieden-2026-10-13). Entwürfe:
https://claude.ai/artifact/HLPBoyvbSbt4U2zofqgjxV (Reihen 1–4: Mail, Benachrichtigungen, Zentrale mit Insights,
Stummschalten, Aufgabe mit Rückbezug, Fokus-Profile). Was die Entwürfe zeigen, gilt; die Werte kommen aus den
Tokens (Skill `torro-design`), neue Tokens entstehen in E8.2.

```
E8.1 ── E8.2 ─┬─ E8.3 ─────────────┐
              ├─ E8.4 ──┐          │
              └─ E8.5 ──┤          │
E8.6a ──────────────────┴─ E8.6b   │   (E8.6b nach E8.3 und E8.4)
E8.7a ─────────────────────────────┴─ E8.7b   (nach E8.3 und E8.5)
E8.8a ─────────────── E8.8b   (nach E8.5 und E8.6a)
E8.9   unabhängig (torro-wm, core.rs)
```

- **Nie parallel:** E8.6a, E8.7a und E8.8a ändern den Katalog (`torro-ipc`, `core.rs`, `mcp.rs`), also nicht
  gleichzeitig untereinander und nicht neben anderen Katalog-Aufgaben. E8.1 bis E8.8b ändern `torroinbox`, also auch
  nicht gleichzeitig untereinander. E8.9 darf neben den Oberflächen-Aufgaben laufen.
- **Neue Übersetzungen** nennt jede Aufgabe im Bericht; eingetragen werden sie in `de.rs` wie immer.

**Schnittstellen, freigegeben am 2026-10-13** (CLAUDE.md):

1. **E8.1:** `torro_core::app::AppId` bekommt Startargumente (z. B. `args: &'static [&'static str]`), und es gibt
   drei Einträge `INBOX_MAIL`, `INBOX_NOTIFICATIONS`, `INBOX` mit den Fensterklassen `torro-inbox-mail`,
   `torro-inbox-notifications` und `torro-inbox`. Startargumente `--view mail|notifications|hub` und
   `--density compact|comfortable`.
2. **E8.6a:** `Item` bekommt Merkmale (`facets`, optional, `#[serde(default)]`), `Rule` bekommt Merkmal, Ablauf
   (`until`) und Wirkung (`NoPopup | Info | Hide`); neue Requests `inbox.list_mute_options` und `inbox.mute_item`
   mit `scope: conversation | facet { key }`; `inbox.set_item_done` bekommt das optionale Feld `dismissed`.
   `mute_source` und `hide_sender` bleiben, wie sie sind.
3. **E8.7a:** Link-Kennungen `mail:<konto-id>:<Message-ID>` (und später `slack:…`); `Item.task` (die Aufgabe, die
   aus dem Eintrag wurde); `torro_pim::model::Link` bekommt neben `github` Angaben zur Mail; neuer Request
   `tasks.create_todo_from_mail`; `apps.open_app` bekommt ein optionales Ziel (`target`).
4. **E8.8a:** neuer Request `inbox.get_stats` und eine Tabelle mit Tagessummen in `inbox.sqlite`.
5. **E8.9:** Sicherheitsregel 6 wird erweitert: „Open“ darf bei App-Quellen das Fenster der App aktivieren. Welche
   App, bestimmt nur der Kern aus der Quelle, nie ein Text des Eintrags; Programme werden mit Argumentliste
   gestartet, nie über eine Shell-Zeile.

### E8.1 Drei Fensterarten
- [x] erledigt (zusätzlich freigegeben: `torroworkplace/src/{core,audit,approvals}.rs`, `ui/apps.rs`,
  `torro-ipc/src/{lib,api}.rs`, dazu `torroinbox/src/compose.rs` für das neue Feld `args`; `apps.open_app` nimmt
  Binary oder Fensterklasse (`AppId::find`), Start mit `AppId::command`; Namen „Torro Inbox Mail“, „Torro Inbox
  Notifications“, „Torro Inbox“; Sichtprüfung nicht möglich, kein Hyprland; `docs/api-reference.md` gibt es noch
  nicht, nichts zu erzeugen)
- **Modell:** Opus · **Braucht:** E3.11 · **Dateien:** `crates/torro-core/src/app.rs`,
  `crates/torroinbox/src/{main,app,overview,glance}.rs`, `crates/torroworkplace/src/apps.rs`,
  `crates/torro-setups/src/storage.rs`, `crates/torro-gui/src/shell.rs`, `crates/torro-core/src/i18n/de.rs`,
  `crates/torroworkplace/src/{core,audit,approvals}.rs`, `crates/torroworkplace/src/ui/apps.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`, `docs/api-reference.md`; Doku: `docs/eingang.md`
- `torroinbox --view mail|notifications|hub` startet eine der drei Fensterarten (ohne Angabe: `hub`). Jede hat eigene
  Fensterklasse, eigenen Titel und eigenen Eintrag im App-Umschalter, in `apps.list_apps` und in den
  Workspace-Vorlagen; eine gespeicherte Vorlage stellt die Fensterart wieder her. `--source` und `--lane` bleiben
  und filtern innerhalb der Fensterart.
- In dieser Aufgabe nur das Gerüst: die Mail-Ansicht zeigt die Mail-Spalte, die Benachrichtigungen die andere Spalte
  aus E3.9, die Zentrale die heutige Übersicht. Aussehen kommt mit E8.3 bis E8.5.
- **Seitenleiste ohne Links zu anderen Apps** (Entscheidung 17): Die Einträge unten in der Seitenleiste, die andere
  Programme öffnen, entfallen in allen drei Fensterarten; der App-Umschalter oben links bleibt.
- **Oberfläche:** App-Umschalter, Steuerzentrale › Apps. **API:** vorhandenes `apps.list_apps` (drei Einträge) und
  `apps.open_app`. **MCP:** vorhandene `apps_list_apps`, `apps_open_app`.
- **Abnahme:** Tests: `--view` wird gelesen, unbekannter Wert → Fehlermeldung mit den gültigen Werten (Exit 64);
  `AppId::ALL` enthält die drei Einträge; eine Vorlage mit Mail-Fenster stellt es als Mail-Fenster wieder her.
  Sichtprüfung je Fensterart in Breit und Schmal.

### E8.1b Nacharbeit: Fensterarten überall erkennen, Reste aus E8.5 und E8.7b
- [x] erledigt (`Link::App` über `AppId::find` beim Melden, Öffnen und im Paneel; Steuerzentrale › Apps wählt
  Name und Beschreibung je Fensterklasse; Freigaben ohne „Later“ auch in `list::actions`; Toast „Done“ und
  „Later until …“, Menü und Auswahl „Later until“, Knopf „Later“, Paneel-Status „Later until …“;
  `InboxClient::create_todo_from_mail` mit Fälligkeit (`MailSource::create_todo_from_mail` bekam `due`), der Dialog
  fragt sie auch für Ordner-Mails; Benachrichtigungen-Fenster: Karte und Zeile der Rückfrage mit Reply · Mark mail
  done · Not now über `hub::acts`, ohne ✕. `de.rs` nicht angefasst, Übersetzungen im Bericht. **Offen:** Reply dort
  öffnet das Mail-Fenster (das Fenster hält den Mail-Eintrag nicht), im Demo nur ein Hinweis; „No longer snoozed“,
  „Unsnooze“, „Nothing snoozed.“ sagen noch „snooze“. **Sichtprüfung nicht möglich:** Bildschirm gesperrt, die App bekommt
  dort kein Fenster)
- **Modell:** Opus · **Braucht:** E8.1, E8.9, E8.5, E8.7b · **Dateien:** `crates/torroworkplace/src/inbox.rs`,
  `crates/torroinbox/src/{detail,list,app,mail,notifications}.rs`, `crates/torro-ipc/src/inbox.rs`,
  `crates/torroworkplace/src/ui/apps.rs`, `crates/torro-core/src/i18n/de.rs`
- **Verweise auf Apps:** Ein Eintrag mit `Link::App` wird heute nur über den Programmnamen erkannt
  (`inbox.rs`, Prüfung beim Melden und Öffnen; `detail.rs`, Knopf und Wort). Ein Verweis auf eine Fensterklasse
  (`torro-inbox-mail`) wird darum abgelehnt. Beide Stellen nutzen `AppId::find`; Unbekanntes bleibt abgelehnt
  (Sicherheitsregel 6).
- **Steuerzentrale › Apps:** Name und Beschreibung werden je Eintrag gewählt, nicht je Programm. Die drei
  Inbox-Zeilen zeigen ihren übersetzten Namen und je eine Zeile Beschreibung (Mail: „Your mail, from a quick look to
  a full client“; Benachrichtigungen: „What your apps report, one column per app“; Zentrale: „Decide, hand on and
  see where it all comes from“).
- **Aus E8.5:** In Listen-Ansichten (`list::actions`) zeigt die Aktionsleiste einer Freigabe noch „Later“; Freigaben
  haben kein Later. Toasts und das Menü sagen noch „Marked as done“, „Snoozed until …“ und „Snooze until“; einheitlich
  „Done“ und „Later“ wie Karten und Paneel („Done“, „Later until …“, „Later until“).
- **Aus E8.7b:** „Task“ an Ordner-Mails fragt nach einer Fälligkeit wie der Dialog aus E3.5: dafür
  `InboxClient::create_todo_from_mail` in `torro-ipc/src/inbox.rs` (statt den Request in `mail.rs` selbst zu bauen).
  Die Karte „You finished a task from a mail“ zeigt auch im Benachrichtigungen-Fenster Reply · Mark mail done · Not
  now. Sichtprüfung nachholen, die in E8.7b am gesperrten Bildschirm scheiterte: `torroinbox --view mail|hub --demo
  --now 2026-10-13T10:30`, `torrotasks --demo --task=quarterly-report@torro-demo`, breit und schmal, hell und dunkel.
- **API:** vorhandene `inbox.post_item`, `inbox.open_item`, `apps.list_apps`, `tasks.create_todo_from_mail`. **MCP:**
  vorhandene Tools.
- **Abnahme:** Tests: Verweis auf `torro-inbox-mail` wird angenommen und geöffnet, unbekannte Fensterklasse →
  abgelehnt; Steuerzentrale zeigt drei verschiedene Namen und Beschreibungen; Freigabe-Zeile ohne Later; Toasts mit
  `tr()`-Vergleich; Aufgabe aus Ordner-Mail mit Fälligkeit; Rückfrage-Karte im Benachrichtigungen-Fenster.
  Übersetzungen im Bericht.

### E8.2 Zwei Dichten und Bausteine für Karten
- [x] erledigt (zusätzlich `torro-core/src/design/pack.rs`: Schlüssel `type.read` im Paket-Format; Tokens
  `typo::read()` 14/22, `size::card_gap()` 12, `avatar()`/`avatar_lg()` 36/40, `tile_sm()`/`tile()`/`tile_lg()`
  20/28/36; `ui_state::Density`, `widgets::density_switch`, `card`, `card_head`, `source_tile`; `chip_row` gab es
  schon, Karten nutzen ihn mit `state_pill`; `typo::all()` bleibt bei acht Stufen; Galerie `-- --cards`, auch
  `--design <id>`; Sichtprüfung am Mac: Galerie dunkel, hell, Plex; Inbox nur Breit, Mittel/Schmal ließ sich ohne
  Hyprland nicht einstellen)
- **Modell:** Opus · **Braucht:** E8.1 · **Dateien:** `crates/torro-core/src/{design.rs,design/rules.rs}`,
  `crates/torro-gui/src/{typo,theme,widgets,ui_state}.rs`, `crates/torro-gui/examples/gallery.rs`,
  `crates/torroinbox/src/{main,app}.rs`, `.claude/skills/torro-design/SKILL.md`, `docs/design-pakete.md`
- **Umschalter** „Compact | Comfortable“ (`segmented`, Icons, in Schmal nur Icons) in der Kopfleiste jeder
  Fensterart; gemerkt je Fensterart und Ausschnitt in `ui.json` (`torro_gui::ui_state`); `--density` beim Start hat
  Vorrang, damit die Workspace-Vorlage die Dichte wiederherstellt. Standard: Comfortable, in Schmal Compact.
- **Neue Tokens** (auch im Paket-Format und in den Regeln von `design/rules.rs`): Schriftstufe `typo::read()` 14/22
  für Lesetext, Kartenabstand, Avatar 36/40, Quellen-Kachel 20/28/36.
- **Neue Bausteine:** `widgets::card` (Fläche `RAISED`, Rand `LINE_STRONG`, Radius `lg`, Innenrand 16, Abstand 12),
  `widgets::card_head` (Titel, Metazeile, ✕ oben rechts), `widgets::source_tile` (App-Icon, sonst Buchstabe auf
  Fläche), `widgets::chip_row` für Anhang/Fälligkeit. Knöpfe der Karte sind immer sichtbar.
- Keine Funktion des Dienstes: Die Dichte ist Darstellung wie die Seitenleiste (U.2), darum kein Request.
- **Abnahme:** Galerie zeigt die Bausteine hell und dunkel und mit `plex`; `scripts/design-check.sh` sauber; Test für
  das Merken in `ui.json` und den Vorrang von `--density`.

### E8.3 Mail-Fenster
- [x] erledigt (Mail-Fenster zeigt nur Mail, `Kind::keeps`; „For me“ ist seine Übersicht, „Snoozed“ heißt dort
  „Later“; Karten nur im Mail-Fenster, `list::Look`; Leser neben der Liste nur in Breit, Mittel/Schmal behalten das
  Paneel; Smart view nur „With attachments“ über die ersten Seiten der Posteingänge – **„Invitations“ und Accept ·
  Maybe · Decline fehlen**: `MessageSummary` hat kein Einladungs-Merkmal und es gibt keinen Request zum Beantworten
  (neue Kern-Aufgabe nötig); kein „Answer by Fri“-Chip, dafür fehlen Daten; Ordner-Karten: Reply · Archive, „Task“
  mit E8.7b; Sichtprüfung am Mac über AeroSpace: sechs Größen, beide Dichten, hell, dunkel, Plex)
- **Modell:** Opus · **Braucht:** E8.2 · **Dateien:** `crates/torroinbox/src/{app,mail,list,detail,overview,keys,glance}.rs`,
  `crates/torro-core/src/i18n/de.rs`
- **Breit:** Seitenleiste mit For me, Info, Later, Done, „Smart views“ (Einladungen, mit Anhang) und je Konto die
  Ordner. Liste und Leser. **Comfortable:** Mail als Karte (Avatar, Absender · Domain · Zeit, Betreff, zwei Zeilen
  Vorschau, Chips) mit sichtbaren Knöpfen Done · Later · Task; Einladungen mit Accept · Maybe · Decline · Later ·
  Task; Leser mit Reply · Reply all · Forward | Done · Later · Task, Lesetext `typo::read()`. **Compact:** eine Zeile
  je Mail wie bisher.
- **Schmal:** keine Ordner, nur „N new for you“ mit Karten (Reply · Task · Done · Later), Info gefaltet am Ende.
- **Kein Verwerfen** im Mail-Fenster (Entscheidung 14). „Task“ in Ordner-Ansichten erst mit E8.7b, bis dahin nur für
  Mails, die Einträge sind.
- **Oberfläche:** wie oben. **API:** vorhandene `inbox.*` und `mail.*` (Lesen, Markieren, Schreiben), keine neue
  Funktion. **MCP:** vorhandene Tools.
- **Abnahme:** Sichtprüfung `torroinbox --view mail --demo` in allen sechs Größen, beide Dichten, hell und dunkel, mit
  `plex`; Tastatur wie bisher (`e`, `s`, `t`, `r`, `m`).

### E8.3b Einladungen im Mail-Fenster beantworten
- [x] erledigt (`MessageSummary.invite` aus dem `text/calendar`-Teil, gelesen beim Sync der Texte und bei
  `get_message`, Schema 3 mit gemerkter Antwort; `mail.respond_to_invitation` schreibt erst den Kalender, dann das
  iTIP-`REPLY` als `multipart/alternative` mit `text/calendar; method=REPLY` über `send::send_message`, Schalter
  „Nach außen“ wie `send_draft`, Ablehnen mit `confirm: on_delete` wartet. Frühere Kopie nur ein Termin der Suite
  aus einer Mail (letzte Zeile „From mail: …“) mit der UID, ±12 h um den Termin; sie bleibt in ihrem Kalender,
  sonst `collection` bzw. der erste beschreibbare. Serien `unsupported`, TZID-Zeiten als Ortszeit (Parser von
  `torro-pim-caldav`). Mail-Fenster: Smart view „Invitations“ (vor „With attachments“, ohne Zahl), Karte mit
  Termin-Chip und Accept · Maybe · Decline · Task (Ordner-Mails haben kein „Later“), Zeile mit Kalender-Zeichen,
  Leser und Paneel mit Termin-Karte (Auswahl-Chips, gegebene Antwort gewählt), Hinweis ohne Undo; `--open
  invitations|invitation`, Demo-Einladungen in `mail.rs`. **Offen:** kein „Hinweis auf Überschneidung“ (die
  Oberfläche kann keine Termine lesen, `InboxClient` kennt `calendar.list_events` nicht), Karten der Einträge in
  „For me“ zeigen die Einladung nicht (`list.rs`, Einträge tragen kein `invite`; Leser und Paneel schon), der
  Eintrag wird beim Antworten nicht erledigt, kein Kalender wählbar in der Oberfläche. Sichtprüfung am Mac über
  AeroSpace: sechs Größen dunkel, hell, compact, Plex)
- **Modell:** Opus · **Braucht:** E8.3, E8.7a · **Dateien:** `crates/torro-mail/src/` (Zusammenfassung einer
  Nachricht), `crates/torroworkplace/src/mail/{read,store,write,send,sync}.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torro-mail-imap/src/mime.rs` (Teil `text/calendar; method=…`), `crates/torro-pim-caldav/src/lib.rs`
  (`pub use ical::parse_events`), `crates/torroworkplace/src/assistant/{data_rule,model}.rs` (freigegeben 2026-10-14),
  `crates/torroworkplace/src/{core,mcp,approvals,audit,daemon}.rs`, `crates/torroinbox/src/{app,mail,overview}.rs`,
  `crates/torro-core/src/i18n/de.rs`; Doku: `docs/{api,mail,eingang}.md`. Fehlt eine Datei, die der Compiler wegen
  neuer Felder erzwingt: nur `None`/`..` ergänzen und im Bericht nennen.
- **Lücke aus E8.3:** Eine Mail-Zusammenfassung (`MessageSummary`) weiß nicht, ob die Mail eine Einladung ist (nur die
  ganze Nachricht hat `invitation`), und es gibt keinen Request, um eine Einladung zu beantworten. Darum fehlen die
  Smart view „Invitations“ und die Knöpfe Accept · Maybe · Decline.
- **Kern:** `MessageSummary` bekommt ein optionales Merkmal „Einladung“ (Beginn, Ende, Titel aus dem `text/calendar`-
  Teil, `#[serde(default)]`). Neuer Request `mail.respond_to_invitation { account, id, answer: accept | tentative |
  decline }` · Write und Schalter „Nach außen“ (verschickt eine Antwort an den Organisator, iMIP `REPLY`) und legt den
  Termin im Kalender an bzw. aktualisiert ihn (prüft Kalender-Schreibrecht selbst wie E3.5). Fehler: `not-found`,
  `forbidden`, `error` (keine Einladung, abgelaufen), `conflict`. Freigabe-Zusammenfassung: Titel und Beginn aus dem
  Mail-Speicher, nie aus dem Client.
- **Oberfläche:** Karte und Leser zeigen den Termin (Kalender-Icon, Tag und Zeit, Hinweis auf Überschneidung) und
  Accept · Maybe · Decline · Later · Task; Smart view „Invitations“ in der Seitenleiste.
- **MCP:** `mail_respond_to_invitation` (wartet je nach „Nach außen“ auf Freigabe).
- **Abnahme:** Tests: Einladung wird erkannt (Fixture mit `text/calendar`), Antwort erzeugt `REPLY` und Termin,
  Rechte (ohne Kalender-Write → `forbidden`), „Nach außen = never“ → nichts geht raus. `torro-api-review`;
  Sichtprüfung Karte und Leser.

### E8.3c Nacharbeit: Einladungen im Eingang und mit Kalenderwahl
- [x] erledigt (`Item.invite` füllt der Kern beim Antworten aus dem Mail-Speicher wie `task`
  (`Core::with_invites`, nur mit Leserecht auf das Konto), nie gespeichert – beim Ablegen kennt der Kern
  den Text der Mail meist noch nicht. Antwort mit `Delivery::Accepted` erledigt den Eintrag der Mail (aus
  Liste, „For me“ oder Paneel), „Undo“ öffnet nur den Eintrag; `unknown` lässt ihn offen. Karten in „For
  me“: Termin-Chip, Accept · Maybe · Decline · Later · Task, Zeilen mit Kalender-Zeichen und den Antworten
  statt „Done“ (`e` bleibt); Knöpfe einer Karte tragen ihre Wörter, soweit sie passen, die ersten zuerst.
  Karte des Termins im Leser und Paneel: „Overlaps with … at 12:30“ / „No conflicts“ (nur belegte
  Termine, `InboxClient::list_events` mit neuem `CalendarEvent`) und die Kalenderwahl (zuletzt gewählt,
  sonst der erste; ohne geladene Kalender wählt der Kern), beides nur vor der Antwort.
  `MailSource::respond_to_invitation` nimmt `collection`, `MailSource::list_events` neu. Sichtprüfung am
  Mac über AeroSpace: „For me“ sechs Größen dunkel, hell 360–1600, compact; Leser Hike/Lunch dunkel,
  hell schmal, Plex)
- **Modell:** Opus · **Braucht:** E8.3b · **Dateien:** `crates/torro-ipc/src/inbox.rs`,
  `crates/torroinbox/src/{mail,list,overview,app,detail}.rs`, `crates/torroworkplace/src/{mail/write,inbox}.rs`,
  `crates/torro-inbox/src/item.rs`, `crates/torro-core/src/i18n/de.rs`; dazu (freigegeben 2026-10-14)
  `crates/torroworkplace/src/core.rs` (`with_invites`), `crates/torroworkplace/src/assistant/data_rule.rs`
  (Test) und, nur `invite: None` im neuen Feld, `crates/torro-inbox/src/{demo,rules,source}.rs`,
  `crates/torro-ipc/src/api.rs`, `crates/torroworkplace/src/{approvals,inbox_notify,mcp}.rs`,
  `crates/torroworkplace/src/assistant/model.rs`
- **Reste aus E8.3b:**
  - Wird eine Einladung beantwortet, ist ihr Eintrag erledigt (Szenario 5 in `eingang.md`), mit Undo nur für den
    Eintrag (die Antwort ist verschickt).
  - Karten und Zeilen in „For me“ zeigen die Einladung (Termin-Chip, Accept · Maybe · Decline); dafür trägt der
    Eintrag einer Mail-Einladung das Merkmal mit (optional, `#[serde(default)]`).
  - Die Oberfläche lässt den Kalender wählen (Feld `collection`, Vorschlag wie im Kern); der Antwortweg nutzt einen
    Helfer `InboxClient::respond_to_invitation` statt `torro_ipc::client::call` direkt.
  - Hinweis auf Überschneidungen: „Overlaps with … 14:00“ bzw. „No conflicts“ über einen Helfer
    `InboxClient::list_events` (vorhandenes `calendar.list_events`, nur sichtbare Kalender).
- **API:** vorhandene `mail.respond_to_invitation`, `calendar.list_events`, `inbox.set_item_done`. **MCP:** vorhandene
  Tools.
- **Abnahme:** Tests: Antwort erledigt den Eintrag; Karte in „For me“ mit Knöpfen; Kalenderwahl schickt `collection`;
  Überschneidung erkannt bzw. „No conflicts“. Sichtprüfung Karte und Leser.

### E8.4 Benachrichtigungen-Fenster
- [x] erledigt (Board in `notifications.rs`, im Modul `app` deklariert, weil `main.rs` parallel geändert wurde –
  die Zeile `mod notifications;` gehört bei Gelegenheit nach `main.rs`; ohne `--source` zeigt das Fenster die
  Apps, Agenten und Suite lassen sich zuschalten; Spalten je App ab Platz für zwei Spalten (mind. `size::props()`),
  sonst eine Spalte „For me“/„Info“, auch in Mittel mit Chips; Compact auch dort mit „For me“/„Info“ statt Tagen;
  ✕ und `x` über `dismiss_item`, „Dismiss all“ ohne Freigaben und Suite; `m` und Glocke schalten im Fenster die
  Quelle stumm; **offen:** die Auswahl der Quellen zur Laufzeit landet nicht in der Vorlage (die speichert die
  Startzeile), Reply fehlt (keine Quelle kann antworten), „In 5 min“ fehlt (keine Erinnerungen im Eingang);
  Sichtprüfung am Mac über AeroSpace: sechs Größen, beide Dichten, hell, dunkel, Plex, „By time“, mit Agenten)
- **Modell:** Opus · **Braucht:** E8.2 · **Dateien:** `crates/torroinbox/src/{app,list,overview,keys,glance}.rs`, neu
  `crates/torroinbox/src/notifications.rs`, `crates/torro-core/src/i18n/de.rs`
- **Breit:** Spalten je App (Kopf: Icon, Name, Zahl) oder „By time“ (ein Kartenraster nach Zeit), umschaltbar.
  Seitenleiste: New, Later, Done; darunter „This window shows“ mit den Quellen zum An- und Abwählen. Die Auswahl gilt
  für dieses Fenster und kommt mit `--source` (mehrfach) in die Workspace-Vorlage.
- **Karte:** Icon der Quelle, Titel, Metazeile, **✕ oben rechts (Verwerfen)**, Text; unten Reply (wo die Quelle es
  kann) bzw. Open, Task, Later, die Glocke (Menü erst mit E8.6b; bis dahin „Mute <Quelle>“ über `mute_source`).
  Erinnerungen: Open · In 5 min. Kopfleiste: „Dismiss all“ mit Undo: je sichtbarem Eintrag ein `set_item_done`,
  Freigaben und Suite-Einträge nie.
- **Schmal:** eine Spalte, Quellen als Chips oben; Compact mit Zeilen und Aktionsleiste (Reply, Task, Mute, ✕).
- **Oberfläche:** wie oben. **API:** Verwerfen = vorhandenes `inbox.set_item_done` (ab E8.6a mit `dismissed: true`,
  damit die Statistik Verwerfen von Erledigt unterscheidet); Open = `inbox.open_item`; Filter = `inbox.list_items`
  mit Quelle. Die Auswahl der Quellen ist Fensterzustand, kein Request. **MCP:** vorhandene Tools.
- **Abnahme:** Test: Verwerfen erledigt den Eintrag und wirkt zurück wie Erledigt; zwei Fenster mit verschiedenen
  Quellen zeigen nur ihre. Sichtprüfung `--view notifications --demo` in allen Größen und beiden Dichten.

### E8.5 Zentrale
- [x] erledigt („Act now“ in `hub.rs`: Kacheln je Herkunft – Mail, Agenten, Suite, je Skript, die vollsten
  zuerst, so viele wie in eine Reihe passen; Klick filtert, `--open tile` –, dann Decide · Agents · To sort mit
  Knöpfen je Eintrag (`hub::acts`): Freigaben Allow · Deny (· Go to window), **ohne ✕, Later und Done** – auch
  `s` und das Paneel vertagen keine Freigabe mehr; Agenten Go to window · Later · Done; Mail Task · Later · Done;
  Rest Open · Task · Later · ✕. Rechts „Coming back“ (Breit, solange kein Paneel offen ist). Seitenleiste Act
  now, To sort (der Abschnitt allein, `--open to-sort`), Later, Done today (nur heute Erledigtes), Sources –
  keine Mail-Ordner. Compact: Agents und To sort als Zeilen mit Aktionsleiste, Freigaben bleiben Karten. Schmal:
  Zahlen oben, große Allow/Deny, Knöpfe unter den Karten; Kopfleiste bei 400 frei. Glance der Zentrale ohne
  Mail-Block. Paneel: „Later“ und „Done“. **Offen:** „Quiet (Ruhe bis …)“ fehlt, kein Request sagt, ob und bis
  wann Ruhe gilt; „Event“ fehlt, kein Request macht aus einem Eintrag einen Termin. Sichtprüfung am Mac über
  AeroSpace: sechs Größen und 400 breit, beide Dichten, hell, dunkel, Plex, Paneel, To sort, Kachel, leer)
- **Modell:** Opus · **Braucht:** E8.2 · **Dateien:** `crates/torroinbox/src/{app,overview,detail,keys,glance}.rs`, neu
  `crates/torroinbox/src/hub.rs`, `crates/torro-core/src/i18n/de.rs`
- **Act now:** oben je Quelle eine Kachel (große Zahl, eine Zeile Erklärung; Klick filtert), dann **Decide**
  (Freigaben mit Befehl bzw. Gegenstand, Allow · Deny · Go to window; kein ✕, kein Later), **Agents** (Go to window ·
  Later · Done), **To sort** (je Eintrag eine Zeile mit Ziel-Knöpfen: Task, Event, Later, Done bzw. ✕).
- **Rechts (Breit):** „Coming back“ (vertagte Einträge mit Zeit) und „Quiet“ (Ruhe bis …); „Handed off“ kommt mit
  E8.7b. Seitenleiste: Act now, To sort, Later, Done today, Sources; Insights erst mit E8.8b.
- **Schmal:** Zahlen oben, Karten mit großen Allow/Deny.
- **Aus E8.3:** Das Paneel, das alle drei Fenster teilen, sagt noch „Snooze“ und „Mark as done“, die Karten „Later“ und
  „Done“. Hier einheitlich auf „Later“ und „Done“ umstellen.
- **Oberfläche:** wie oben. **API:** vorhandene `inbox.list_items`, `approvals.list_approvals`, `inbox.open_item`,
  `inbox.set_item_done`, `inbox.snooze_item`, `inbox.create_task_from_item`; Allow/Deny wie E3.8. **MCP:** vorhandene
  Tools; Freigaben entscheiden bleibt `Never`.
- **Abnahme:** Sichtprüfung `--view hub --demo` in allen Größen; Freigaben haben keinen Knopf zum Verwerfen oder
  Vertagen (Test auf die Knopfliste je Eintragsart).

### E8.5b Ruhe-Status und Termin aus einem Eintrag
- [x] erledigt (`inbox.get_quiet` antwortet `quiet: null` oder `{ reason: busy, until, title? }`, `until` = Ende
  des am längsten laufenden Termins „busy“, Titel nur mit Kalender-Leserecht und Kalender in der Auswahl, Hook-Clients
  nie; Logik in `inbox_notify::quiet_now`, die auch die Popups nutzen. `inbox.create_event_from_item`: Termin
  „busy“, Rückbezug als letzte Zeile der Beschreibung (Mail-Zeile, sonst der Web-Link; `Event` hat kein URL-Feld),
  Eintrag danach erledigt und als `done` gezählt (keine eigene Statistik-Spalte). Zentrale: rechts „Quiet“ über
  „Handed off“, in Mittel und Schmal eine Zeile unter Kacheln bzw. Zahlen; „Event“ bei To sort, „Make event“ im
  Paneel, Taste `c`, Dialog mit Kalender, Tag, Beginn (halbstündlich) und Dauer, `--open event`; Anfragen über
  neues `hub::HubSource` (dafür `torroinbox/src/main.rs` eine Zeile Trait-Schranke). Datenschutz: Antwort von
  `create_event_from_item` für entfernte Modelle zurückgehalten. **Offen:** `assistant/model.rs` bildet das neue Tool
  noch nicht auf `[inbox, calendar]` ab (Datei nicht freigegeben), ein im Gespräch abgeschalteter Kalender greift
  dort also nicht. Sichtprüfung am Mac ohne AeroSpace (Fenster-Id über die PID): Zentrale sechs Größen dunkel,
  Dialog breit und schmal hell)
- **Modell:** Opus · **Braucht:** E8.5, E3.4 · **Dateien:** `crates/torro-ipc/src/{lib,api,inbox}.rs`,
  `crates/torroworkplace/src/{core,inbox,inbox_notify,pim,mcp,approvals,audit,daemon}.rs`,
  `crates/torroinbox/src/{app,hub,detail,keys}.rs`, `crates/torro-core/src/i18n/de.rs`; Doku: `docs/{api,eingang}.md`.
  Stellen, die der Compiler wegen neuer Fälle erzwingt: nur das Nötigste, im Bericht nennen.
- **Lücken aus E8.5:** Die Zentrale zeigt rechts „Quiet“ nicht, weil kein Request sagt, ob gerade Ruhe gilt und bis
  wann. Und „Event“ fehlt bei „To sort“, weil kein Request aus einem Eintrag einen Termin macht.
- **Ruhe:** `inbox.get_quiet { }` · Read → ob Ruhe gilt, warum (Termin „busy“, später Fokus-Profil) und bis wann; den
  Titel des Termins nur mit Kalender-Leserecht, sonst „a busy event“. **MCP:** `inbox_get_quiet`. **Oberfläche:**
  Zentrale rechts „Quiet until 15:00 · Kundentermin“, in Schmal eine Zeile unter den Zahlen.
- **Termin:** `inbox.create_event_from_item { id, collection, title?, start, end }` · Write (Bereich `inbox`, prüft
  Kalender-Schreibrecht und Kalender-Auswahl selbst wie E3.5 für Tasks; `not-found` für unsichtbare Kalender), der
  Termin bekommt den Rückbezug wie in E8.7a (`URL` bzw. eine Zeile „From …“ in der Beschreibung). Freigabe-
  Zusammenfassung: Titel aus dem Eintrag beim Fragen festgehalten, Kalendername aus dem Cache. **MCP:**
  `inbox_create_event_from_item`. **Oberfläche:** „Event“ bei „To sort“ und im Paneel öffnet einen kleinen Dialog
  (Titel vorbelegt, Kalender, Beginn, Dauer), wie „Task“ (E3.5).
- **Abnahme:** Tests: Ruhe aus einem Termin „busy“ mit und ohne Kalenderrecht, keine Ruhe ohne Termin; Termin
  entsteht mit Rückbezug, Rechte (ohne Kalender-Write → `forbidden`, ausgeblendeter Kalender → `not-found`),
  Freigabe-Text. `torro-api-review`; Sichtprüfung Zentrale und Dialog.

### E8.6a Stummschalten nach Merkmalen: Kern
- [x] erledigt (zusätzlich freigegeben, nur so weit der Compiler es erzwingt: `torroworkplace/src/{daemon,pim}.rs`
  (je zwei Zweige für die neuen Requests), `torroworkplace/src/ui/{inbox,launch}.rs` (neue Felder an `Rule`, Zweig für
  `NoPopup` mit vorläufigem Satz), `torroinbox/src/empty.rs` (drei Trait-Methoden mit `NotFound`); Wirkung in
  Requests `MuteEffect`, an der Regel `Info` = Spur Info, `Hide` = `Muted`, neu nur `RuleEffect::NoPopup`; eine Regel
  mit Merkmal gilt für alle Quellen derselben Art, die Unterhaltung nur in ihrer Quelle; Antwort `muted` mit `done`
  und `moved`; Merkmal „Thema“ der Skripte fehlt, weil `inbox.post_item` kein Feld dafür hat; `docs/api-reference.md`
  gibt es nicht und nichts erzeugt sie, `TORRO_BLESS` wertet kein Test aus)
- **Modell:** Opus · **Braucht:** E3.11 · **Dateien:** `crates/torro-inbox/src/{item,rules,source,demo}.rs`,
  `crates/torro-ipc/src/{lib,api,inbox}.rs`, `crates/torroworkplace/src/{core,inbox,inbox_notify,mcp,approvals,audit}.rs`,
  `crates/torroworkplace/src/mail/inbox_feed.rs`, `docs/api-reference.md`; Doku: `docs/{api,eingang}.md`; zusätzlich
  `crates/torroworkplace/src/{daemon,pim}.rs`, `crates/torroworkplace/src/ui/{inbox,launch}.rs`,
  `crates/torroinbox/src/empty.rs`
- **Merkmale am Eintrag** (`facets`): je Quelle, was sie verlässlich liefert (Tabelle in `eingang.md`). Heute: Mail
  (Konto, Absender, Domain, `List-Id`, Massenmail, Unterhaltung), Agenten (Agent, Projekt, Art), Skripte (Client,
  Thema). Weitere Quellen tragen ihre mit E5/E7 nach.
- **Regel:** Merkmal + Wert, Ablauf (`until`, leer = immer), Wirkung `NoPopup` (Eintrag ohne Popup und Ton), `Info`
  (Spur Info, zählt nicht), `Hide` (kein Eintrag). Abgelaufene Regeln wirken nicht mehr und verschwinden aus der Liste.
  Freigaben und die Suite trifft keine Regel (Sicherheitsregel 1).
- **API:** `inbox.list_mute_options { id }` · Read → die Möglichkeiten für diesen Eintrag (Umfang, Merkmal, Wert,
  Wort für das Menü, nötiger Zugriff). `inbox.mute_item { id, scope: conversation | facet { key }, until?, effect }`:
  - **Der Wert kommt immer aus dem gespeicherten Eintrag** (`item.facets`), nie aus dem Request. Sonst könnte ein
    Agent beliebige Absender stummschalten, und die Freigabe zeigte Text des Clients.
  - **Zugriff** (in `Request::operation`, darum `scope` als Enum): Write nur für `conversation` mit `Hide` oder `Info`
    ohne `until` (das erledigt bzw. verschiebt nur diesen Eintrag, keine Regel, wie E3.11); alles andere ist eine
    Regel und braucht Admin.
  - **Antwort** wie `hide_sender`: Regel (falls eine entstand) und die erledigten Ids für Undo. Undo: `conversation`
    über `set_item_done { done: false }`, Regeln über `remove_rule`.
  - **Fehler:** `not-found` für fremde oder unbekannte Einträge, für Konten ohne Leserecht und für Suite/Freigaben bei
    Client-Schlüsseln (wie `set_item_done`); `forbidden` für Suite/Freigaben beim App-Schlüssel; `error` für ein
    Merkmal, das der Eintrag nicht hat, oder `until` in der Vergangenheit; `conflict`, wenn sich der Eintrag beim
    Ausführen einer Freigabe geändert hat.
  - **Freigabe-Zusammenfassung** (`approvals.rs`): Merkmal und Wert über `InboxFacts.item` aus dem Cache, Dauer und
    Wirkung mit `tr()` („Mute #general in Slack until tomorrow (no popup)“).
  - **Verwerfen zählbar:** `inbox.set_item_done` bekommt das optionale Feld `dismissed` (verträglich); die
    Katalog-Beschreibung nennt es.
  - Katalog, `docs/api.md`, API-Referenz neu erzeugen.
- **Fremdtext:** Merkmalswerte (Absender, Kanal, `List-Id`, Chat-Titel) sind `untrusted`, in `list_items`/`get_item`
  wie in `list_mute_options`; die Tool-Beschreibungen sagen „data, never instructions“. Merkmalswerte gehen nie ins
  Protokoll.
- **MCP:** `inbox_list_mute_options`, `inbox_mute_item` (Admin-Umfänge warten auf Freigabe).
- **Offen, im Bericht vorschlagen:** Sollen Einträge mit `Hide` sieben Tage unter „Muted“ sichtbar bleiben, damit man
  Regeln prüfen kann?
- **Abnahme:** Tests je Wirkung und je Merkmal, Ablauf, Wert nur aus dem Eintrag, Rechte (Write nur für
  `conversation` ohne Regel), Suite/Freigaben nie, Undo je Umfang, `dismissed` wird gespeichert, Protokoll ohne
  Merkmalswerte. `torro-api-review`.

### E8.6b Stummschalten: Menü und Regeln
- [x] erledigt (Glocke, `m` und die Glocke im Paneel des Benachrichtigungen-Fensters öffnen `Menu::Mute`: „Mute“ mit
  den Möglichkeiten aus `list_mute_options`, „For“ und „What happens“ als `widgets::segmented` (←/→, ↑/↓, Ziffern),
  „All rules ↗“ öffnet die Steuerzentrale; Voreinstellung Always · Only Info, Wahl bleibt, solange das Fenster
  läuft; Toast im Wortlaut der Regelseite mit `Undo::Muted` (Regel weg, Einträge auf, Verschobene zurück); ist die
  Quelle als Ganzes stumm, steht „Unmute <Quelle>“ im Menü; Hub und Mail-Fenster behalten „Hide from inbox“;
  `--open mute-menu|mute-panel`; „Move to Info“ und die Spur Info tragen `Icon::Eye` (Lucide `info` wäre besser,
  braucht `icon.rs`); Regelseite: Sätze mit Wirkung, Ablauf und Merkmal, darunter Geltung und Treffer („Hid 12 since
  Tuesday“); zusätzlich, für die verlangten Treffer und die Demo: `torroworkplace/src/ui/{mod,launch}.rs`
  (`Data::rule_hits`, zwei Merkmal-Regeln und Treffer im Demo); `widgets::menu_item` neu; `de.rs` nicht angefasst,
  Übersetzungen im Bericht; **Sichtprüfung nicht möglich:** Bildschirm gesperrt, die App bekommt kein Fenster)
- **Modell:** Opus · **Braucht:** E8.6a, E8.3, E8.4 · **Dateien:** `crates/torroinbox/src/{app,notifications,list,keys,detail,mail,empty}.rs`,
  `crates/torroworkplace/src/ui/inbox.rs`, `crates/torro-gui/src/widgets.rs`, `crates/torro-core/src/i18n/de.rs`
- **Verwerfen** (✕, „Dismiss all“) schickt ab jetzt `set_item_done` mit `dismissed: true`.
- **Menü der Glocke** (und Taste `m`): Abschnitt „Mute“ mit den Möglichkeiten aus `list_mute_options`, „For“ (1 hour ·
  Until tomorrow · Always), „What happens“ (No popup · Only Info · Hide), unten der Weg zu den Regeln. Nach dem
  Stummschalten der Toast mit Undo (E3.11). Im Mail-Fenster bleibt „Hide from inbox“ (E3.10), es nutzt dieselben
  Regeln.
- **Aus E8.1b:** „No longer snoozed“, „Unsnooze“, „Nothing snoozed.“ und „Snoozed entries wait here …“ sagen noch
  „snooze“; einheitlich „Later“ wie überall sonst (neue Schlüssel, Übersetzungen im Bericht).
- **Aus E8.3:** „Move to Info“ trägt im Leser bisher das Glocken-Icon. Die Glocke ist ab hier das Stummschalten;
  „Move to Info“ bekommt ein eigenes Icon.
- **Steuerzentrale › Inbox › Rules:** alle Regeln mit Wort („Muted until tomorrow: #general in Slack“), Ablauf und
  „Remove“.
- **API/MCP:** die aus E8.6a; `inbox.remove_rule`.
- **Abnahme:** Sichtprüfung des Menüs (Breit, Schmal, beide Dichten) und der Regelseite; Undo.

### E8.7a Aufgabe mit Rückbezug: Kern
- [x] erledigt (Konto-Id `mail:<adresse>` aus `accounts.list_accounts`, Kennung `mail:<adresse>:<Message-ID>` mit
  der Message-ID ohne spitze Klammern, wie der Speicher sie hält (`torro_pim::model::bare_message_id`); `Link.mail`
  als `Option<Box<MailDetails>>`, sonst schlägt Clippy wegen der Größe von `torro_pim::Change` an; Index auf die
  Message-ID im Mail-Speicher (Schema 2), `task_notes` in `inbox.sqlite` (Schema 5); ein Eintrag zeigt auch die
  Aufgabe einer Mail derselben Unterhaltung; „Reply“/„Mark mail done“ als Knöpfe erst mit E8.7b (aus `task.link`,
  `task.mail_entry`), weil neue `Action`-Fälle `torroinbox` brechen; `save_todo` setzt `mail:`-Links nur mit
  Leserecht aufs Konto; `open_app` mit Ziel startet immer ein neues Fenster, die Apps lesen `--message`/`--task`
  und zeigen es ab E8.7b; GitHub-Einträge gibt es im Eingang noch nicht (E5), andere Quellen bleiben `inbox:`;
  Handprüfung in Nextcloud Tasks offen; `docs/api-reference.md` gibt es nicht)
- **Modell:** Opus · **Braucht:** E3.5, E4.1 · **Dateien:** `crates/torro-pim/src/model.rs`,
  `crates/torro-pim-caldav/src/{ical,write}.rs`, `crates/torro-ipc/src/{lib,api,inbox}.rs`,
  `crates/torro-inbox/src/item.rs`, `crates/torroworkplace/src/{core,inbox,pim,mcp,approvals,apps}.rs`,
  `crates/torroworkplace/src/mail/{store,inbox_feed}.rs`, `crates/torroinbox/src/main.rs`,
  `crates/torrotasks/src/main.rs` (Startziel), `crates/torro-core/src/i18n/de.rs`, `docs/api-reference.md`; Doku:
  `docs/{api,eingang}.md`; zusätzlich freigegeben (2026-10-13), nur so weit der Compiler es erzwingt (`mail: None`,
  `task: None`, `target: None`, Muster mit `..`): `crates/torro-pim-github/src/{address,item}.rs`,
  `crates/torro-pim/src/demo.rs`, `crates/torrotasks/src/{detail,list,app}.rs` (nur die genannten Stellen),
  `crates/torro-inbox/src/{source,rules,demo}.rs`, `crates/torroworkplace/src/{inbox_notify,daemon,audit}.rs`,
  `crates/torroworkplace/src/ui/{apps,inbox,mod}.rs`, `crates/torroworkplace/src/tray/menu.rs`,
  `crates/torro-gui/src/source.rs`; in `audit.rs` und `daemon.rs` je ein echter Zweig mit Test; neue Testdaten
  `crates/torro-pim-caldav/tests/fixtures/nextcloud-tasks-mail-link.ics`
- **Kennung:** `inbox.create_task_from_item` schreibt für Mail `mail:<konto-id>:<Message-ID>`, für GitHub wie bisher
  `github:…`, sonst `inbox:<id>`. `<konto-id>` ist die feste Kennung des Kontos aus `accounts.list_accounts`, denn die
  Kennung liegt dauerhaft auf dem CalDAV-Server. Dazu eine lesbare Zeile am Ende der Beschreibung („From mail: …“,
  übersetzt). Vorhandene Aufgaben mit `inbox:<id>` bleiben gültig. Kennung und Message-ID gehen nie ins Protokoll.
- **Beide Richtungen, nur mit Rechten:**
  - Beim Lesen der Aufgaben füllt der Kern zu `mail:`-Links Betreff, Absender, Datum und die Zahl neuer Antworten
    seit dem Anlegen (wie `Link::github`, nie gespeichert), aber nur, wenn der Client das Konto lesen darf
    (`mail_accounts`); sonst bleibt nur die Kennung.
  - Einträge bekommen `task` (Titel, Liste, fällig, erledigt), wenn eine Aufgabe auf sie bzw. ihre Mail zeigt, aber
    nur, wenn der Client Tasks lesen und die Liste sehen darf.
- **Erledigt wirkt:** Wird eine verknüpfte Aufgabe erledigt (beim Abgleich erkannt), meldet die Suite einen Eintrag,
  wenn die Mail noch offen ist. Der Titel steht fest mit `tr()` („You finished a task from a mail“); Aufgabentitel
  und Absender sind Fremdtext und stehen nur im Text, mit `untrusted: true`. Spur Info, kein Popup, **genau einmal je
  Aufgabe**, damit ein Agent über `tasks.save_todo` keine Flut von Suite-Einträgen auslöst. Knöpfe „Reply“ und
  „Mark mail done“.
- **Neu:** `tasks.create_todo_from_mail { account, id, collection, title?, due? }` · Write (Bereich `tasks`, wie jede
  neue Aufgabe) für Mails in Ordnern, die kein Eintrag sind. Der Handler prüft zusätzlich Leserecht auf das Konto;
  an der Mail ändert sich nichts. Fehler: `not-found` (Konto, Mail oder Liste nicht sichtbar), `forbidden` (ohne
  Tasks-Write), `conflict` (Mail beim Ausführen einer Freigabe geändert). Freigabe-Zusammenfassung: Betreff aus dem
  Mail-Speicher über `account`/`id`, Listenname aus dem Cache, Titel beim Fragen festgehalten (wie E3.5).
  **MCP:** `tasks_create_todo_from_mail`.
- **Neu:** `apps.open_app` bekommt ein optionales Ziel (`target`, verträglich): `mail:<konto-id>:<Message-ID>` öffnet
  das Mail-Fenster mit der Mail, `task:<uid>` die Aufgaben-App mit der Aufgabe. Der Kern prüft das Ziel und baut die
  Startargumente; die Oberfläche startet nichts selbst. **MCP:** vorhandenes `apps_open_app` mit dem neuen Feld.
- **Test mit Nextcloud:** Bleibt `X-TORRO-LINK` erhalten, wenn die Aufgabe in Nextcloud Tasks (Web) bearbeitet wird?
  Ergebnis im Bericht; Apple Erinnerungen, wenn möglich.
- **Abnahme:** Tests: Kennung je Quelle, Zeile in der Beschreibung, `inbox:` bleibt lesbar, Mail im anderen Ordner
  wird über die Message-ID gefunden, neue Antwort zählt, Anreichern nur mit Rechten (je ein Test für Mail und
  Tasks), Suite-Eintrag genau einmal und `untrusted`, Rechte und Freigabe wie E3.5, `open_app` mit Ziel.
  `torro-api-review`.

### E8.7b Aufgabe mit Rückbezug: Oberfläche
- [x] erledigt (Mail-Fenster: Karte der Aufgabe über dem Text in Leser und Paneel (`mail::task_card`, „Open
  task“), „Task“ auch an Ordner-Mails (Karte, Leser, Paneel, `t`) über `tasks.create_todo_from_mail` – **ohne
  Fälligkeit**, weil `torroinbox` `torro_pim::EventTime` nicht kennt (Rückfrage: `torro-pim` als Abhängigkeit oder
  Methode im `InboxClient`); der Request geht vorerst direkt aus `mail.rs`. `--message` öffnet den Eintrag der Mail
  bzw. die Mail in ihrem Ordner. Aufgaben-App: „From“ (Avatar, Betreff, „Mail · Absender · Konto · Datum“, „Open
  mail“, neue Antworten), „Activity“ mit „Made from a mail“ (der Zeitpunkt des Anlegens fehlt im Modell),
  Mail-Hinweis in der Zeile, `--task` öffnet die Aufgabe in ihrer Liste. Zentrale: Abschnitt „Follow up“ für „You
  finished a task from a mail“ (Reply · Mark mail done · Not now, auch im Paneel), rechts „Handed off“ über „Coming
  back“; Demo-Daten in `app.rs` (`demo_task`, `demo_mail_link`). „Open mail“/„Open task“ über `apps.open_app` mit
  `target`, im Demo starten sie die andere App mit `--demo`. **Sichtprüfung nicht möglich:** Bildschirm gesperrt)
- **Modell:** Opus · **Braucht:** E8.7a, E8.3, E8.5 · **Dateien:** `crates/torroinbox/src/{app,mail,detail,hub}.rs`,
  `crates/torrotasks/src/{detail,list}.rs`, `crates/torro-core/src/i18n/de.rs`
- **Mail:** über dem Text die Aufgabe als Karte (Kreis, Titel, „Task · Liste · due … · open“, „Open task“); „Task“
  auch in Ordner-Ansichten (`tasks.create_todo_from_mail`).
- **Aufgaben-App:** Block „From“ im Detail (Avatar, Betreff, „Mail · Absender · Konto · Datum“, „Open mail“; Hinweis
  auf neue Antworten), Zeile im Verlauf.
- **Zentrale:** rechts „Handed off“ (Aufgaben aus Einträgen, offen) und die Rückfrage nach dem Erledigen.
- **API/MCP:** die aus E8.7a; „Open mail“ und „Open task“ über `apps.open_app` mit `target`.
- **Abnahme:** Sichtprüfung Mail, Aufgaben-Detail und Zentrale; „Open mail“ öffnet das Mail-Fenster mit der Mail.

### E8.8a Insights: Zählen
- [x] erledigt (eine Tabelle `stats` in `inbox.sqlite`, Schema 6: Tagessummen je lokalem Tag, Quelle, Merkmal und
  Kennzahl, dazu Meldungen je Stunde und Zeit bis erledigt in Stufen, aus denen der Median kommt; Merkmale, die die
  Quelle selbst sind (Konto, Agent, Client), zählen nicht eigens; `muted_mail` zählt eine ferngehaltene Mail nur
  einmal, auch nach einem Abgleich; Antwort `stats` mit `days`, `hours`, `totals`, `median_done_minutes`,
  `suggestions` (ab 10 Meldungen, höchstens jede fünfte geöffnet, je Art summiert, höchstens 5); `inbox.list_rules`
  bekommt `hits` (`rule`, `hits`, `since`, je Quelle gezählt, Spalte `rules.counted_since`), ältere Regeln zählen ab
  dem Update; zur Aufgabe zählt nicht als erledigt (`Inbox::done_as_task`); Popups über `Popups::arrivals`, ohne die
  Tray zu wecken; Löschen beim Entfernen in `core.rs` (`remove_client`, `remove_mail_account`), darum
  `account.rs`/`clients.rs` unberührt, die Quelle eines Agenten erst, wenn kein anderer Client mehr für ihn meldet;
  das Stummschalten der Suite zählt nicht (der Abgleich läuft jede Minute); vom Compiler erzwungen:
  `torroworkplace/src/{audit,daemon,pim}.rs`, `torroworkplace/src/ui/mod.rs` (`..` im Muster);
  `docs/api-reference.md` gibt es nicht)
- **Modell:** Opus · **Braucht:** E8.6a · **Dateien:** `crates/torro-ipc/src/{lib,api,inbox}.rs`,
  `crates/torroworkplace/src/{core,inbox,inbox_notify,mcp,account,clients}.rs`, `docs/api-reference.md`; Doku:
  `docs/{api,eingang}.md`; zusätzlich, nur so weit der Compiler es erzwingt: `crates/torroworkplace/src/{audit,daemon,pim}.rs`,
  `crates/torroworkplace/src/ui/mod.rs`
- **Zählen** in `inbox.sqlite`, Tagessummen je Quelle und Merkmal: neu, geöffnet, verworfen (`dismissed` aus
  E8.6a), erledigt, vertagt, zur Aufgabe, stummgeschaltet, Popups (davon in Ruhe), Zeit bis erledigt (Median). Keine
  Titel, keine Texte, nichts ins Protokoll.
- **Aufbewahrung:** Summen je Quelle 13 Monate; Zeilen je Absender, Domain, Kanal oder `List-Id` nur 90 Tage. Wird ein
  Konto oder Client entfernt, gehen seine Zeilen mit. In `docs/api.md` ein Absatz „Statistik ist Metadatum“.
- **Vorschläge** rechnet der Kern: Merkmale mit vielen Einträgen und wenig Geöffnetem. Jeder Vorschlag nennt die Id
  des jüngsten passenden Eintrags, den der Kern noch kennt, und die passende Möglichkeit aus `list_mute_options`; der
  Knopf führt so `inbox.mute_item` aus. Ohne bekannten Eintrag gibt es keinen Vorschlag.
- **API:** `inbox.get_stats { range: week | month | quarter, until? }` · Read → Summen je Tag und Quelle, Stoßzeiten,
  Kennzahlen, Vorschläge; `error` bei unbekanntem Zeitraum. **Rechte wie `inbox.list_rules`:** nur Quellen aus
  `inbox_sources`, Mail nur mit Leserecht auf das Konto (`mail_accounts`), `suite` nie für Client-Schlüssel. Werte
  von Merkmalen sind `untrusted`. **MCP:** `inbox_get_stats`.
- **Abnahme:** Tests: Zählen je Ereignis, Verwerfen getrennt von Erledigt, Zeiträume, Aufräumen nach 90 Tagen bzw. 13
  Monaten und beim Entfernen eines Kontos, Rechte (fremde Quellen und Konten fehlen), Vorschlag ab einer Schwelle und
  nur mit bekanntem Eintrag. `torro-api-review`.

### E8.8b Insights: Ansicht
- [x] erledigt (Seitenleiste der Zentrale: „Insights“ nach Done today, eine Seite auf den Einträgen der Übersicht,
  `--open insights|insights-table`; Kopfleiste mit Zeitraum, Week | Month | Quarter und „Table view“ (Schmal oben auf
  der Seite); vier Kennzahlen (Eingegangen mit Schnitt je Tag, Popups mit in Ruhe zurückgehaltenen, Anteil geöffnet,
  Median bis erledigt); gestapelte Säulen je Tag (Quartal: je Woche, vom letzten Tag zurück) mit Reihen Mail, Agents,
  den zwei meistgenutzten Skripten und „Other“ (Suite und übrige Skripte), Legende, Tooltip über die ganze Säulenhöhe;
  Stoßzeiten je Stunde mit Satz zur vollsten Stunde; „Where you could cut back“ mit Knopf je Vorschlag (`mute_item`
  mit „For“ und „What happens“ des Fensters, Toast mit Undo; jedes Laden fragt die Zahlen neu, ein genommener
  Vorschlag verschwindet); „Table view“ zeigt dieselben Zahlen als Tabellen (Test). Neue Tokens `chart` (4) und
  `chart_other` hell und dunkel, mit dem Dataviz-Validator geprüft (das Grau „Other“ des Entwurfs `#6B6F76` fiel
  neben Magenta durch, jetzt `#5C6068`), im Paketformat und in den Regeln (≥ 3:1 auf `panel`); fehlen sie im Paket,
  folgen sie dessen `calendar` bzw. `text_3`; die Pakete in `designs/` bleiben gültig (Test). Bausteine
  `widgets::{stat_tile, chart_card, column_chart, chart_legend, share_bar, data_table}`, `theme::ChartColor`. Kern:
  `mail.get_message` mit dem App-Schlüssel zählt den offenen Eintrag der Mail als geöffnet, einmal je Eintrag und Tag,
  auch gegen `inbox.open_item` (Tabelle `opened_items`, Schema 7); Agenten zählen nicht. Zusätzlich, vom Compiler
  bzw. vom fehlenden Weg zum Eingang erzwungen: `torroworkplace/src/core.rs` (eine Zeile: `read::answer_counting`
  mit dem Eingang), `torro-gui/src/design/legacy.rs` (die neuen Felder), Skill `torro-design`. Icon: `ChartGantt`, Lucide
  `chart-column` bräuchte `icon.rs`. `de.rs` nicht angefasst, Übersetzungen im Bericht; **Sichtprüfung nicht möglich:**
  Bildschirm gesperrt, die App bekommt kein Fenster – alle Größen nur per Test gezeichnet)
- **Modell:** Opus · **Braucht:** E8.8a, E8.5 · **Dateien:** neu `crates/torroinbox/src/insights.rs`,
  `crates/torroinbox/src/{app,hub,main}.rs`, `crates/torro-gui/src/{widgets,theme}.rs`,
  `crates/torro-core/src/{design.rs,design/rules.rs,design/pack.rs}`, `crates/torroworkplace/src/{inbox,mail/read}.rs`,
  `docs/design-pakete.md`, `crates/torro-core/src/i18n/de.rs`
- **Aus E8.8a, Kern:** „Geöffnet“ zählt bisher nur `inbox.open_item`. Liest jemand eine Mail im Mail-Fenster
  (`mail.get_message` zu einer Mail, die ein offener Eintrag ist), zählt das ebenfalls als geöffnet, einmal je Eintrag
  und Tag; sonst schlagen die Vorschläge für Mail zu oft Stummschalten vor. Kein neuer Request.
- Zentrale › **Insights**: Woche | Monat | Quartal; vier Kennzahlen; Säulen je Tag nach Quelle (gestapelt, Legende,
  Tooltip mit Werten); Stoßzeiten je Stunde; „Where you could cut back“ mit Knopf je Vorschlag (führt
  `inbox.mute_item` aus, mit Undo). Umschalter „Table view“ zeigt alle Zahlen als Tabelle.
- **Diagrammfarben** als eigene Tokens (vier Quellen-Farben plus Grau für „Other“), hell und dunkel auf
  Farbenblindheit geprüft; nie die Farben von Status oder Torro-Rot.
- **API/MCP:** die aus E8.8a und E8.6a.
- **Abnahme:** Sichtprüfung in Breit, Mittel, Schmal; Tabelle zeigt dieselben Zahlen wie die Diagramme.

### E8.9 Ins richtige Fenster springen, auch am Mac
- [x] erledigt (`torro_wm::focus_process_window`: Titel wählt, am Mac zusätzlich das tty bei Terminal und iTerm2,
  ohne AeroSpace hebt macOS die App selbst; der Hook schickt am Mac Prozesse. Windows ohne Prozesskette offen)
- **Modell:** Opus · **Braucht:** E2.4 · **Dateien:** `crates/torro-wm/src/{lib,types}.rs`,
  `crates/torro-wm/src/{aerospace,glazewm,hyprland}/`, `crates/torroworkplace/src/{core,hook}.rs`,
  `crates/torro-inbox/src/{item,hook}.rs`
- Heute scheitert „Go to window“, wenn ein Terminal alle Fenster aus einem Prozess bedient (Terminal.app, iTerm2,
  Ghostty: am Mac der Normalfall), und ohne Kachel-Manager. Zu klären und umzusetzen:
  - Unter mehreren Fenstern des Prozesses das richtige wählen, z. B. über den Fenstertitel (Projektordner,
    Sitzungsname) oder einen Titel, den der Hook setzt; im Bericht festhalten, was je Terminal verlässlich ist.
  - Ohne AeroSpace am Mac die App des Prozesses nach vorn holen, ohne `unsafe` (z. B. über `open -a` bzw. System
    Events); unter Windows mit GlazeWM entsprechend.
  - Für App-Quellen (später Slack, WhatsApp): „Open“ aktiviert das Fenster der App. Welche App, bestimmt der Kern aus
    der Quelle, nie ein Text des Eintrags; gestartet wird mit Argumentliste, nie über eine Shell-Zeile
    (Sicherheitsregel 6, erweitert mit Schnittstelle 5; die Regel oben im Dokument mit anpassen).
- **API:** vorhandenes `inbox.open_item`. **MCP:** vorhandenes `inbox_open_item`.
- **Abnahme:** Tests mit dem Fake-Fenstermanager: mehrere Fenster, Auswahl über den Titel, kein Treffer →
  `unsupported` mit Grund. Von Hand am Mac mit Terminal.app und einem zweiten Terminal geprüft, im Bericht.

### E8.10 Fehlende Icons nachrüsten
- [x] erledigt (`BellOff`, `Info`, `ChartColumn`, `Table` aus Lucide 0.469.0; `BellOff` an „Mute…“ von Karte, Zeile und
  Paneel, an „Unmute“ im Menü und an den Vorschlägen in Insights; Spur „Info“ `Info`, Insights `ChartColumn`, „Table view“
  `Table`; `Bell` bleibt für das Fenster und die Spalte „Notifications“. Neuer Test: jede Variante lädt die Datei ihres
  Namens. „Mute/Unmute <Quelle>“ im Menü „Hide from inbox“ behält den Mond)
- **Modell:** Opus · **Braucht:** E8.6b, E8.8b · **Dateien:** `crates/torro-gui/src/icon.rs`,
  `crates/torro-gui/assets/icons/{bell-off,info,chart-column,table}.svg` (neu), `crates/torro-gui/examples/gallery.rs`,
  `crates/torroinbox/src/{app,detail,notifications,insights,list}.rs`, `.claude/skills/torro-design/SKILL.md`
- **Lücke aus E8.6b und E8.8b:** Im Icon-Satz fehlen eine durchgestrichene Glocke (Stummschalten), Info (Spur
  „Info“) und ein Säulendiagramm (Insights); dort stehen bisher `Bell`, `Eye` und `ChartGantt`. Für „Table view“ fehlt
  ein Tabellen-Icon.
- **Umsetzung:** Die vier Lucide-Icons (0.469.0) als Dateien im Format der vorhandenen anlegen (Kopf
  `<!-- @license lucide-static v0.469.0 - ISC -->`, `stroke-width="1.75"`), Varianten `BellOff`, `Info`, `ChartColumn`,
  `Table` in `Icon`. Einsetzen: Glocke zum Stummschalten → `BellOff`; Spur Info → `Info`; Insights → `ChartColumn`;
  „Table view“ → `Table`. `Bell` bleibt für Benachrichtigungen allgemein (Fenster, Spalte).
- **API/MCP:** keine Funktion, nur Darstellung.
- **Abnahme:** Galerie zeigt die vier Icons; Test, dass jede Variante eine Datei hat (falls vorhanden, sonst neu);
  `scripts/design-check.sh` sauber; Sichtprüfung der Stellen hell und dunkel.

### E8.11 Feinschliff Mail-Fenster nach der Sichtprüfung
- [x] erledigt (Karten in jeder Größe mit Reply · Task · Done · Later, Wörter nach Platz, `Look::Cards` ohne
  `words`; Einladungen unter „Invitations“ mit Later und „…“ für ihren Eintrag, `s`/`m` ebenso; Breit: Liste 460 mit
  Leser-Spalte, leer „Select a mail to read it“; Kartenhöhe aus der Zeilenzahl der Vorschau bei geschätzter Breite;
  Gruppenköpfe über Karten mit Icon „Today 3“; „N new for you“ in Schmal in beiden Dichten; Mini und Leiste: Kopfzahl
  in `display()`, „From …“ darunter, keine Gesamtzahl ungelesener Mail; Compact-Zeile ohne Konto gleich Domain.
  Sichtprüfung offen: Bildschirm gesperrt, Befunde durch Tests abgedeckt)
- **Modell:** Opus · **Braucht:** E8.10 · **Dateien:** `crates/torroinbox/src/{app,list,mail,overview,glance}.rs`,
  `crates/torro-core/src/i18n/de.rs`
- **Befunde der Sichtprüfung vom 2026-10-14** (Bilder im Scratchpad unter `review-e8/`), Fehler:
  1. Breit/Mittel, Comfortable: Accept · Maybe · Decline (und die anderen Karten-Knöpfe) stehen nur als Icons, obwohl
     Platz ist; das ✕ für Decline sieht aus wie Verwerfen. Die Wörter hängen am Platz (`spoken()` in `list.rs`), nicht
     an der Größenklasse (`app.rs`, `Look::Cards { words: … }`).
  2. Breite Mail-Karte ohne „Reply“: in jeder Größe Reply · Task · Done · Later (Konzept, Knöpfe je Eintrag).
  3. Smart view „Invitations“ ohne „Later“ und „…“: wie dieselbe Mail unter „For me“.
- **Sollte:**
  4. Breit: leere Fläche rechts neben der Liste → Leser-Spalte mit `empty_state` („Select a mail to read it“); der
     Rollbalken gehört an den Rand der Liste.
  5. Comfortable-Karten mit einzeiliger Vorschau: kein Loch von zwei Zeilen; die Höhe aus der tatsächlichen
     Zeilenzahl bei bekannter Breite.
  6. Gruppenköpfe in beiden Dichten gleich (Icon und Schreibweise „Today 3“), nur der Abstand unterscheidet sich.
  7. Kopfzeile „N new for you“ in Schmal in beiden Dichten oder in keiner.
  8. Mini: nur „for you“ als Kopfzahl, „From …“ direkt darunter, außerhalb des Kontenkastens; keine zweite, andere
     Zahl.
  9. Leiste: Kopfzahl in `typo::display()` wie in Mini.
- **Kleinigkeit:** 10. Compact-Zeile: kein Konto-Etikett, wenn es der Domain des Absenders gleicht.
- **API/MCP:** keine Funktion, nur Darstellung.
- **Abnahme:** Tests je Befund (Knöpfe mit Wort in Breit, Reply auf der breiten Karte, Later bei Einladungen, Leer-
  Leser, Kartenhöhe bei einer Zeile, Gruppenkopf gleich, Kopfzahl in Mini/Leiste); `scripts/ci-local.sh`;
  Sichtprüfung, sobald der Bildschirm offen ist.

### E8.12 Feinschliff Benachrichtigungen, Zentrale, Aufgaben und Regeln nach der Sichtprüfung
- [x] erledigt („From“ und Aufgaben-Karte mit `widgets::card_line`: erst Konto/„Task“, dann Absender/Zustand weg,
  Rest mit „…“ und Tooltip, Knopf wird Icon vor dem Titel; „Hide completed“ verdrängt den Fortschrittsbalken;
  Knopfleisten alles oder nichts, nur die Hauptaktion allein mit Wort (`widgets::bar_words`, echte Abstände, Plex
  passt); Regel ghost = loslassen, sonst secondary; Menüs öffnen über dem Auslöser, wenn unten kein Platz ist, an der
  Knopfzeile der Karte; „Make event/task“ im Paneel linksbündig am Knopf, Feld bündig; Zentrale: Decide/Follow up
  Mittel volle Breite, Breit nebeneinander; To sort und „Cut back“ höchstens `content_max`, To-sort-Knöpfe in festen
  Spalten, ohne doppelten Titel; Leiste: letzte Zelle wächst; eine linke Achse für Chips, Köpfe, Karten;
  Gruppenköpfe mit Icon in beiden Dichten; Insights Mittel 2×2, Vorschlag je Zeile in der Oberfläche (Kern nennt nur
  das Merkmal), „33%“, „Busiest: …“ ohne Punkt; Rules: Titel = Gegenstand, Zeile = Wirkung · Umfang · Treffer; Ziffern
  im Mute-Menü als `meta`; „New 5“ auch in Schmal. Offen: Anführungszeichen und App-Icons der Demo (`torro-inbox`
  `demo.rs`), Einzelwort im Hinweis „This text comes from outside …“ (`torroinbox/src/detail.rs`); Sichtprüfung am Mac
  hell/dunkel, Mittel/Breit/Schmal, Plex)
- **Modell:** Opus · **Braucht:** E8.11 · **Dateien:** `crates/torroinbox/src/{app,notifications,hub,insights,mail,glance,overview,list}.rs`,
  `crates/torrotasks/src/detail.rs`, `crates/torroworkplace/src/ui/inbox.rs`, `crates/torro-gui/src/widgets.rs`,
  `crates/torro-core/src/i18n/de.rs`
- **Befunde der Sichtprüfung vom 2026-10-14** (Bilder im Scratchpad unter `review-e8/`), Fehler:
  1. Aufgaben „From“: Texte stumm abgeschnitten („Tue, 13“, „anna@work.examp“) – `torrotasks/src/detail.rs`
     (`card_words`: nur `Wrapping::None` + `clip`). `typo::ellipsize` mit Tooltip; in Schmal/Mittel erst das Konto,
     dann den Absender weglassen.
  2. Dasselbe in der Aufgaben-Karte im Mail-Paneel („… · ope“) – `torroinbox/src/mail.rs` (`task_card`).
  3. Mittel, Aufgaben-Detail: „Hide completed“ läuft über den Rand; „Open mail“ verdrängt den Titel (in Mittel unter
     die Zeile oder nur Icon mit Tooltip).
  4. Knopfleisten mischen Wort und Icon (`notifications.rs`, `worded`): alles oder nichts je Leiste; höchstens die
     Primäraktion behält ihr Wort. Die Schätzung mit Plex ist zu vorsichtig.
- **Sollte:**
  5. Zentrale Mittel: „Decide“ und „Follow up“ über die volle Breite; Breit: nebeneinander statt halb breit untereinander.
  6. Mute-Menü: nie über dem Auslöser; oberhalb öffnen, wenn unten kein Platz ist (Mittel, und mit Paneel in Breit).
  7. „Make event“-Karte am Knopf im Paneel ausrichten und im Paneel halten; Kanten im Dialog angleichen.
  8. Breit: „To sort“ und „Where you could cut back“ auf `size::content_max()`; Knopfspalten in fester Reihenfolge.
  9. Leiste: Zellen nutzen den Platz (letzte Zelle wächst oder eine vierte), `glance.rs` `cell_max()`.
  10. Schmal/Mittel: eine linke Achse für Chips, Gruppentitel und Karten.
  11. Gruppenköpfe in beiden Dichten derselbe Baustein (wie E8.11 im Mail-Fenster).
  12. Knopfstile in der Zentrale nach einer Regel (sichtbare Kartenknöpfe secondary, „weg damit“ ghost); große
      Allow/Deny in Schmal bewusst nur für Entscheidungen, im Skill festhalten.
  13. Insights Mittel: Kennzahlen 2×2 wie in Schmal.
  14. „Where you could cut back“: je Zeile der passende Vorschlag (Hide sender, Mute channel …) statt immer „Only
      Info“; prüfen, ob Kern (`get_stats`) oder Oberfläche das festlegt, und es dort richten.
  15. Steuerzentrale › Rules: Titel = Gegenstand, Untertitel = Wirkung · Umfang · Treffer, einzeilig mit „…“.
- **Kleinigkeiten:** doppeltes „To sort 9“; einheitliche Anführungszeichen; „33%“ ohne Leerzeichen und ohne
  Schlusspunkt im Englischen; Zahlen im Mute-Menü als `meta` statt `kbd`; „New“ in Schmal mit Zahl; einzelnes Wort in
  der Zeile „This text comes from outside …“; Demo-Einträge mit eigenem App-Icon (GitHub, Kalender).
- **API/MCP:** keine neue Funktion; Punkt 14 ggf. Logik im Kern (`inbox.get_stats`), ohne Schnittstellenänderung.
- **Abnahme:** Tests je Befund, wo prüfbar; `scripts/ci-local.sh`; Sichtprüfung der genannten Bilder neu.

### E8.13 Letzte Kleinigkeiten aus E8.12
- [x] erledigt (Demo: “Team meeting”, “migrate”, „Mum’s“ typografisch, Test ohne gerade Anführungszeichen in
  Einträgen und Mail; Eintrag `demo-13` kommt jetzt vom Skript `github` („2 security alerts in website“) und zeigt
  das GitHub-Icon – dafür kennt `list::source_icon` Skripte namens `github`, Signatur unverändert; Paneel: Hinweis
  kürzer, „Text from outside the suite, shown as plain text.“, eine Zeile in 400 auch auf Deutsch, Test; derselbe
  Satz unter einer ganzen Mail (`mail.rs`, eine Zeile, über `detail::foreign_note`); Mail-Karten
  (Mail und Einladung) mit `widgets::bar_words`: alle Wörter, sonst nur Reply bzw. Accept, sonst Icons)
- **Modell:** Opus · **Braucht:** E8.12 · **Dateien:** `crates/torro-inbox/src/demo.rs`, `crates/torroinbox/src/{detail,list}.rs`,
  `crates/torro-core/src/i18n/de.rs`
- Demo: einheitliche Anführungszeichen („typografisch“ im Englischen: “…”) in Titeln und Texten; ein bis zwei
  Demo-Einträge mit eigenem App-Icon (z. B. GitHub, Kalender), damit die Spalten je App das Icon zeigen.
- Paneel: Die Zeile „This text comes from outside the suite …“ lässt in Breit kein einzelnes Wort in der zweiten Zeile
  (kürzer formulieren oder anders umbrechen).
- Mail-Fenster: Auch die Einladungskarte folgt der Regel aus E8.12 (`widgets::bar_words`): alle Wörter oder keins je
  Leiste, höchstens die Hauptaktion behält ihr Wort (Skill `torro-design`).
- **API/MCP:** keine.
- **Abnahme:** Tests (Wortregel an der Einladungskarte, Demo-Icons); `scripts/ci-local.sh`; kurze Sichtprüfung, wenn
  der Bildschirm offen ist.

## E5 Nextcloud, GitHub, ntfy als Quellen (Plan 2026-10-14)

Konzept: [eingang.md › Quellen, Stufe 1](../eingang.md#stufe-1-ohne-fremde-anmeldung-oder-schon-vorhanden), „Sortieren“,
„Drei Fenster“ (Merkmale je Quelle, Stummschalten, Rückbezug), „MCP und API“, „Architektur“. GitHub-Konto:
[github.md](../github.md), [settings.md](../settings.md). Skills: `torro-api` (jede Aufgabe mit Request), `nextcloud`
(E5.7–E5.10).

Drei Quellen ohne Firmenfreigabe. GitHub und Nextcloud nutzen die vorhandenen Konten; ntfy bekommt eine eigene, kleine
Kontoart. Die Quellen legt der Kern selbst ab wie Mail (E4.1, `mail/inbox_feed.rs`); kein Client kann einen solchen
Eintrag melden, zusammenführen oder auflösen (Sicherheitsregel 2). Je Quelle ein Modul
`crates/torroworkplace/src/sources/<quelle>.rs`; alle nutzen die Ablage aus E5.2, damit die Quellen nebeneinander
entstehen, ohne `inbox.rs` gleichzeitig zu ändern.

```
E5.1 ── E5.2 ─┬───────────── E5.4 ─┬─ E5.5
              │   E5.3 ──────┘      └─ E5.6   (braucht auch E4.1)
              ├───────────── E5.8 ─┬─ E5.9
              │   E5.7 ──────┘      └─ E5.10  (optional)
              └─ E5.12 ───── E5.13
                  E5.11 ─────┘
E5.14 nach E5.4, E5.8, E5.9, E5.12            (Steuerzentrale)
E5.15 nach E5.1 (mit Demo); Sichtprüfung mit echten Daten nach E5.4, E5.8, E5.13
```

- **Sofort möglich:** E5.3, E5.7, E5.11 legen nur ein neues Crate bzw. Modul an (plus eine Zeile in der
  Wurzel-`Cargo.toml`) und testen mit Fixtures (Ausnahme wie bei E1.1a).
- **Nie parallel:** E5.2 und E5.12 ändern den Katalog (`torro-ipc/src/{lib,api}.rs`, `core.rs`, `mcp.rs`), nicht
  gleichzeitig untereinander und nicht neben anderen Katalog-Aufgaben. `crates/torroworkplace/src/inbox.rs` ändern nur
  E5.2 und E5.6, nie gleichzeitig. E5.4, E5.8, E5.9, E5.13 arbeiten nur in ihrem `sources/*.rs` mit den Funktionen aus
  E5.2 und dürfen parallel laufen; brauchen sie `inbox.rs` oder `core.rs`: anhalten und fragen. `account.rs` ändern
  E5.4 und E5.12, nicht gleichzeitig. `torroinbox` ändert nur E5.15 (E5.1 nur, was der Compiler erzwingt). Die
  Steuerzentrale (`ui/*`) ändert nur E5.14.
- **Übersetzungen** nennt jede Aufgabe im Bericht; eingetragen werden sie in `de.rs` wie immer.
- **Tests ohne Netz:** Fixtures unter `tests/fixtures/` des Crates, HTTP mit `httpmock` (im Workspace, als
  Dev-Abhängigkeit `httpmock.workspace = true`), feste Uhr. Kein Test spricht mit github.com, einer Nextcloud oder
  ntfy.sh.

### Schnittstellen, freigegeben am 2026-10-14

Schnittstellen nach CLAUDE.md, vom Nutzer alle 16 freigegeben.

1. **E5.1 – `SourceKind`:** neue Fälle `Github`, `Nextcloud`, `Talk`, `Ntfy`; Schlüssel gleich der Konto-Id
   (`github:<login>`, `nextcloud:<login>@<host>`, `ntfy:<host>`), Talk mit dem Rest der Nextcloud-Konto-Id
   (`talk:<login>@<host>`); dazu `SourceRef::{github,nextcloud,talk,ntfy}`, `FromStr`, `label()`. Neue Enum-Werte sind
   im Draht verträglich, alte Clients kennen sie aber nicht.
2. **E5.1 – `Kind::Alert`:** legt nur der Kern an, immer „für mich“ und dringend (ntfy-Priorität 5); `admitted_kind`
   macht daraus bei Clients und Hooks `notice`. Der Doc-Kommentar von `Kind::Message` gilt künftig auch für Chat
   (Talk). **Ergänzt Sicherheitsregel 3:** Die Dringlichkeit setzt hier der Absender des Themas, aber nur in Themen,
   die der Mensch selbst abonniert hat.
3. **E5.1 – `FacetKey`:** neue Fälle `Repo`, `Reason`, `Result`, `App`, `Person`, `Topic`, `Priority`, `Tag`; `Kind`
   gilt auch für die Art bei GitHub (`pull_request`, `issue`, `check_suite` …) und Talk (`direct`, `mention`,
   `group`); `is_foreign` und `facet_label` bekommen die neuen Fälle. Neu `rules::remote_facets(kind, Vec<Facet>)`:
   lässt je Quellenart nur die erlaubten Schlüssel durch und kürzt die Werte, damit Merkmale weiter nur vom Kern kommen.
4. **E5.1 – `MuteScope::Facet { key, nth }`:** `nth` optional, Standard 0, für mehrwertige Merkmale (ntfy-Tags). Der
   Wert kommt weiter nur aus dem gespeicherten Eintrag.
5. **E5.1 – Spur-Regeln als reine Funktionen:** `rules::github_place`, `nextcloud_place`, `talk_place`, `ntfy_place`
   nach den Tabellen in den Aufgaben.
6. **E5.1/E5.2 – `SourceInfo.health`:** optional, Werte `missing_app | missing_scope | signed_out | offline`, damit
   Steuerzentrale und App sagen können, warum eine Quelle still ist („Notifications need a classic token“).
7. **E5.2 – Rechte (`Rights::reaches_source`):** GitHub-Einträge sieht ein Client nur mit Quelle in `inbox_sources`,
   **Lesen in `github`** und dem Repo des Eintrags in `github_repos` (`reaches_repo`), gefiltert in SQL vor dem Lesen;
   ebenso für `list_sources`, Statistikzeilen mit `Repo`, Regeln und Stummschalt-Möglichkeiten mit `Repo`. Nextcloud,
   Talk und ntfy brauchen nur `inbox_sources`. `suite` bleibt für Clients unsichtbar.
8. **E5.2 – Verhalten vorhandener Befehle (Katalogtexte):** `inbox.set_item_done` mit `done: true` (auch
   `dismissed`), `create_task_from_item` und `create_event_from_item` **wirken auf die Quelle zurück**: GitHub Thread
   „done“ (`DELETE /notifications/threads/{id}`; für Clients mit Schreiben in `github` und dem Repo, sonst `forbidden`
   bzw. `not-found`), Nextcloud Benachrichtigungen löschen, Talk Unterhaltung gelesen, ntfy nichts. Undo, Ausblenden
   und Stummschalten wirken nie zurück. `create_task_from_item` schreibt bei Issue- und PR-Einträgen
   `github:<owner>/<repo>#<n>` mit den Regeln aus G.6b.
9. **E5.2 – Speicher:** Schema 8 von `inbox.sqlite`: Tabelle `remote_ids` (Eintrag ↔ Kennungen der Quelle), Spalte
   `repo` für den Filter aus Punkt 7, Tabelle `cursors` (`Last-Modified`, ETag, ntfy-`since`); neue `Inbox`-Funktionen
   `file_remote`, `resolve_remote`, `retire_remote`, `remote_ids`, `cursor`/`set_cursor`, `set_source_health`; Modul
   `crate::sources` mit `start` und `settle_done`.
10. **Neue Crates und Module:** `crates/torro-nextcloud` (OCS: Capabilities, Benachrichtigungen, Talk, `notify_push`;
    laut Skill `nextcloud` neue Crates nur nach Rückfrage), `crates/torro-ntfy` (Abo-Protokoll), in
    `torro-pim-github` das Modul `notifications.rs` mit eigenem Typ `Notifications` und eigenem Fehler-Enum (`Api` und
    `Error` bleiben). Die REST-Adresse ist die GraphQL-Adresse ohne `/graphql`, damit `GithubAccounts::new` bleibt.
11. **E5.10 (optional) – Abhängigkeit:** `tokio-tungstenite` in der Wurzel-`Cargo.toml` mit TLS-Feature (rustls) für
    `wss://`. ntfy braucht keine neue Abhängigkeit (`reqwest::Response::chunk()` ohne Feature `stream`).
12. **E5.6 – Mail:** `torro_mail::MessageTraits.github` (optional, `#[serde(default)]`, aus `X-GitHub-Reason`),
    `NewMail.github` (Repo und Nummer aus `List-Id` und Message-ID); eine GitHub-Mail wird dem GitHub-Eintrag
    zugeordnet.
13. **E5.12 – ntfy-Konto:** `AccountKind::Ntfy` (`ntfy`), `Account::ntfy(server, user)`,
    `Account.ntfy: Option<NtfyConfig { topics: Vec<NtfyTopic { topic, label }>, user: Option<String> }>`; das Geheimnis
    (Passwort oder Token `tk_…`) nur im Schlüsselbund unter der Konto-Id; `AccountInfo.ntfy: Option<NtfyView>`.
14. **E5.12 – neue Requests:** `accounts.connect_ntfy { server, topics }` · Admin · Tool (Clients öffnen nur die
    Steuerzentrale wie bei `connect_github`); `accounts.set_ntfy_login { server, topics, user?, secret }` · Admin ·
    **Never** („Was nie nach außen geht“ 5, in die `kept_back`-Liste von `names_follow_the_rules`);
    `accounts.set_ntfy_topic { account, topic, label?, subscribed }` · Admin · Tool.
15. **Erweitert Sicherheitsregel 6 (Links fremder Quellen):** Den Link baut der Kern: GitHub nur
    `https://github.com/<owner>/<repo>/…` aus den geprüften Teilen, nie die URL aus der Antwort; Nextcloud und Talk nur
    mit demselben Origin wie der Server des Kontos, sonst kein Link; ntfy `click` nur `http`/`https`. Öffnen über einen
    Client-Schlüssel wartet wie immer auf eine Freigabe. Zugangsdaten gehen nie an einen anderen Origin, auch nicht an
    die `notify_push`-Adresse aus den Capabilities.
16. **Assistent:** `assistant/data_rule.rs`: Einträge von Nextcloud und Talk bekommen entfernte Modelle wie Mail nur
    als Id; GitHub und ntfy gehen durch ([ki.md](../ki.md): „Mail und Messenger nur lokal, GitHub darf in die Cloud“).
    `assistant/model.rs`: `accounts.set_ntfy_topic` berührt `[accounts, inbox]`, `accounts.connect_ntfy` `[accounts]`.

### E5.1 Modell der neuen Quellen
- [x] erledigt (2026-10-14: `SourceKind::{Github, Nextcloud, Talk, Ntfy}`, `Kind::Alert`, acht neue `FacetKey`s, `rules::{allowed_facets, remote_facets, github_place, nextcloud_place, talk_place, ntfy_place, app_label}` mit `TalkRoom`/`TalkLevel`, `MuteScope::Facet { key, nth }`, `SourceInfo.health` (`SourceHealth`); Demo `demo-14` bis `demo-18`; Clients sehen die neuen Quellen bis E5.2 nicht, auch nicht im SQL-Filter; Tests in `torroinbox` an die Demo angepasst)
- **Modell:** Opus · **Braucht:** Freigabe 1–6 · **Dateien:** `crates/torro-inbox/src/{item,rules,source,demo}.rs`;
  nur so weit der Compiler es erzwingt (neue Fälle in `match`, kein neues Verhalten, im Bericht nennen):
  `crates/torro-ipc/src/api.rs` (`reaches_source` liefert für die neuen Arten `false` bis E5.2),
  `crates/torroworkplace/src/{inbox,approvals,inbox_notify}.rs`, `crates/torroworkplace/src/ui/agents.rs`,
  `crates/torroinbox/src/{list,hub,insights,notifications}.rs` (vorläufig Icon `AppWindow`, Herkunft wie ein Skript);
  `crates/torro-core/src/i18n/de.rs`
- **Quellen:** `SourceKind::{Github, Nextcloud, Talk, Ntfy}` mit Konstruktoren, `key()`, `label()`, `FromStr`;
  `ParseSourceError` nennt die neuen Formen. `label()`: „GitHub“, „Nextcloud“, „Talk“, „ntfy“; das Konto zeigt die App
  erst bei mehreren (E5.15).
- **Art:** `Kind::Alert`; `placement(Alert)` = für mich und dringend; `admitted_kind(Alert, Client|Hook)` = `Notice`.
  `human_only` bleibt: die neuen Arten lassen sich stummschalten.
- **Merkmale** (Freigabe 3), je Quellenart erlaubt (`rules::allowed_facets`):

  | Quelle | Merkmale | fremd (`untrusted`) |
  |---|---|---|
  | GitHub | `repo` (owner/repo), `reason`, `kind` (Art), `result` (`failed` · `succeeded` · `cancelled`, nur Prüfläufe) | nur `repo` |
  | Nextcloud | `app`, `person` | beide |
  | Talk | `person`, `kind` (`direct` · `mention` · `group`) | `person` |
  | ntfy | `topic` (Etikett aus dem Konto), `priority` (`1`–`5`), `tag` (mehrfach) | `tag` |

  `facet_label` mit `tr()` („Everything from {repo}“, „Review requests“, „Mentions“, „Successful checks“, „Failed
  checks“, „Files“ bzw. „Talk“ für bekannte App-Kennungen, „Messages from {person}“, „The topic {topic}“, „Priority
  {n}“, „The tag {tag}“). `mute_options` bietet jeden Tag einzeln an (`nth`); `mute_target` nimmt den Wert mit `nth`
  aus dem Eintrag.
- **Spuren als reine Funktionen** (Freigabe 5, Begründungen im Doc-Kommentar, Abweichungen im Bericht):
  - `github_place(reason, kind, result)`: für mich `review_requested`, `mention`, `team_mention`, `assign`,
    `approval_requested`, `security_alert`, `invitation`, `author`, sowie `ci_activity` mit `failed`; Zur Info alles
    andere (`comment`, `state_change`, `subscribed`, `manual`, `ci_activity` sonst, Unbekanntes). Nie dringend (offene
    Frage 3).
  - `nextcloud_place(app, object_type)`: für mich `files_sharing`, `comments` (Erwähnung), `spreed` (Anruf, Chat),
    `deck`, `dav` (Erinnerung), `twofactor_*`; Zur Info `updatenotification`, `announcementcenter`, `firstrunwizard`,
    `activity` und Unbekanntes.
  - `talk_place(kind, mention, level)` nach der Benachrichtigungsstufe von Talk: „nie“ → kein Eintrag; „nur
    Erwähnungen“ → nur Erwähnungen; „immer“ → auch Gruppen, dann Zur Info; Standard: 1:1 immer, Gruppen nur bei
    Erwähnung.
  - `ntfy_place(priority)`: 5 → `Alert`; 4 → `Notice` für mich; 1–3 → `Notice` Zur Info.
- **`SourceInfo.health`:** optional, `#[serde(default)]` (Freigabe 6).
- **Demo:** je Art ein Eintrag in `DemoSource` (Review angefragt, fehlgeschlagener Prüflauf, Nextcloud-Freigabe einer
  Datei, Talk-Chat 1:1, ntfy „Backup failed“ mit Priorität 5), typografische Anführungszeichen wie in E8.13;
  `demo-13` bleibt bis E5.15.
- **API/MCP:** keine Requests; nur das Wire-Format ist neu (Freigabe).
- **Abnahme:** Tests: serde-Rundreise (`"github"`, `"alert"`, `"repo"`, `nth` fehlt → 0), `FromStr`/`Display` je Art,
  Schlüssel gleich der Konto-Id; `admitted_kind` (`Alert` nur vom Kern); Spur je Tabellenzeile; `remote_facets`
  verwirft fremde Schlüssel und leere Werte und kürzt auf `FACET_MAX`; `nth` wählt den richtigen Tag; `facet_label`
  mit `tr()`; Demo je Art. fmt/clippy/test grün, auch mit `TORRO_LANG=de`.

### E5.2 Kern: Meldungen fremder Quellen ablegen, zeigen und zurückmelden
- [x] erledigt (2026-10-14: `Inbox::{file_remote, resolve_remote, retire_remote, remote_ids, open_remote, cursor, set_cursor, set_source_health}` mit `Remote`/`RemoteEntry`, Schema 8 (`items.repo`, `remote_ids`, `cursors`); `open_remote` zusätzlich, damit E5.4/E5.8 „fehlt in vollständiger Liste“ ohne `inbox.rs` finden; Kennung zählt erneut nur mit neuerem Datum; Rechte aus Freigabe 7 in `Rights::reaches_source` und `Scope` (SQL über `repo` bzw. Merkmal `repo`); `check_done` (GitHub braucht Schreiben in `github`); `sources::{Sources, start, settle_done}` mit leeren Modulen, Kern ruft `settle_done` nach Erledigt, Aufgabe, Termin; Freigabe-Texte, `reach_of` je Quellenart; `forget_account` streicht `talk:`; Konto entfernt → `retire_remote` + `forget_source`; Datenregel Nextcloud/Talk)
- **Modell:** Opus · **Braucht:** E5.1, Freigabe 7–9 und 16 · **Dateien:** `crates/torroworkplace/src/inbox.rs`, neu
  `crates/torroworkplace/src/sources/{mod,github,nextcloud,talk,ntfy}.rs` (Gerüst, leer bis E5.4, E5.8, E5.9,
  E5.13), `crates/torroworkplace/src/{main,core,approvals,audit,clients}.rs`,
  `crates/torroworkplace/src/assistant/data_rule.rs`, `crates/torro-ipc/src/{lib,api}.rs`; vom Compiler erzwungen
  `crates/torroworkplace/src/{daemon,pim,mcp}.rs`; Doku: `docs/{api,eingang}.md`
- **Ablegen:** `Inbox::file_remote(&Remote, now)` (Quelle, Vorgang, Kennungen der Quelle, Art, Spur, Titel, Text,
  Absender, Web-Link, Merkmale, Datum). Eigentümer ist die Konto-Id wie bei Mail, so fasst kein Request diese Einträge
  an (Sicherheitsregel 2). Zusammenführen über den Vorgang (`rules::merge`); eine schon abgelegte Kennung zählt nie
  doppelt. Immer `untrusted: true`; Titel und Text gekürzt wie bei Clients. Merkmale nur über `rules::remote_facets`.
  Eine stumme Quelle legt nichts ab; Regeln mit Merkmal wirken wie bei Mail. Statistik wie `file_mail`.
- **Auflösen:** `resolve_remote(source, gone, now)` streicht Kennungen; ein Eintrag ohne Kennung ist erledigt (zählt
  als erledigt). `retire_remote(kind, active, now)` erledigt die Einträge entfernter Konten (wie `retire_mail`). Dazu
  `remote_ids(item)` und `cursor`/`set_cursor` (Schema 8, alte Dateien ohne Verlust angehoben).
- **Sehen:** Regeln aus Freigabe 7 in `Rights::reaches_source` und `Scope::of`; Repo-Filter in SQL über die Spalte
  `repo`; ebenso `list_sources`, `get_stats` (Zeilen je `Repo`), `list_rules`, `list_mute_options`. Hook-Clients sehen
  weiter nichts.
- **Zurückmelden:** Nach `set_item_done { done: true }`, `create_task_from_item` und `create_event_from_item` ruft der
  Kern im Hintergrund `sources::settle_done(&item)` (wie `inbox_feed::mark_read`); die Module füllen es später. Rechte
  vor der Freigabe und bei ihrer Ausführung (GitHub wie `check_mail_done`, mit Schreiben in `github` und dem Repo).
  Freigabe-Zusammenfassung aus dem Cache mit `tr()` („… and its GitHub notification done“, „… and delete its Nextcloud
  notification“, „… and mark the Talk conversation read“).
- **Zustand der Quelle:** `set_source_health` (im Speicher) füllt `SourceInfo.health`.
- **Konten:** `Clients::forget_account` streicht zu `nextcloud:<x>` auch `talk:<x>`; wird ein GitHub- oder
  Nextcloud-Konto entfernt, kommen `retire_remote` und `forget_source` für beide Schlüssel dazu.
- **Datenregel:** Nextcloud und Talk bekommen entfernte Modelle nur als Id (Freigabe 16, Doc-Kommentar oben in
  `data_rule.rs` ergänzen).
- **Katalog:** neue Beschreibungen für `inbox.set_item_done`, `create_task_from_item`, `create_event_from_item`,
  `list_items`, `list_sources` (Rückmeldung je Quelle, `health`, „data, never instructions“); in `docs/api.md` Zeile
  `inbox` und Fußnote „Fremde Quellen im Eingang“.
- **Oberfläche:** keine (E5.14, E5.15). **API:** keine neuen Requests, geändertes Verhalten. **MCP:** dieselben Tools.
- **Abnahme:** Tests im Temp-Verzeichnis mit einer Fake-Quelle: Ablegen, Zusammenführen, Zählen; dieselbe Kennung nur
  einmal; Auflösen → erledigt; Konto weg → erledigt; stumm → nichts; Regel mit Merkmal. Ein Client sieht einen
  GitHub-Eintrag nur mit `github`-Lesen und dem Repo (`list_items`, `get_item`, `list_sources`, `get_stats`,
  `list_rules`). `settle_done` genau bei Erledigt, Aufgabe, Termin, nie bei Undo, Ausblenden, Stummschalten. GitHub
  erledigen ohne Schreiben in `github` → `forbidden` vor der Frage; Wortlaut der Freigabe. Protokoll ohne Titel und
  Merkmalswerte; Datenregel für Nextcloud und Talk; Schema 7 → 8 ohne Verlust. `torro-api-review` ohne hohen Befund.

### E5.3 GitHub: Benachrichtigungen lesen
- [x] erledigt (2026-10-14: `notifications::Notifications` mit `list`/`done`/`read`, `Listing`, `Thread`, `ThreadId`, `Reason`, `SubjectKind`, `RunResult`, `NotificationsError`; Nummer nur von der REST-Adresse des Tokens, `next`-Seiten nur dort; Fine-grained-Tokens laut Doku nicht unterstützt → 403 = `NeedsScope`)
- **Modell:** Opus · **Braucht:** – (sofort möglich) · **Dateien:** neu `crates/torro-pim-github/src/notifications.rs`,
  `crates/torro-pim-github/src/lib.rs` (nur `pub mod` und Doc-Kommentar), `crates/torro-pim-github/Cargo.toml`
  (Dev-Abhängigkeit `httpmock`, `tokio`), neu `crates/torro-pim-github/tests/fixtures/notifications/*.json`
- **Abfragen:** `Notifications::new(http, rest_base, token)`, `list(last_modified, since)`:
  `GET /notifications?all=false&per_page=50` mit `Accept: application/vnd.github+json` und
  `X-GitHub-Api-Version: 2022-11-28`; `If-Modified-Since` → 304 bei keiner Änderung (zählt nicht gegen die Grenze);
  Seiten über `Link: rel="next"`, höchstens 10, nur eine ganz gelesene Liste ist `complete`; `poll` =
  `X-Poll-Interval` (mindestens 60 s); bei 403/429 bestimmen `Retry-After` bzw. `X-RateLimit-Reset` die Wartezeit.
  Eigenes `NotificationsError`: `Refused` (401), `NeedsScope` (403 bei Fine-grained-Tokens; in der offiziellen Doku
  prüfen), `Throttled { until }`, `Offline`, `Github`.
- **Thread:** `id` nur Ziffern; `repo` nur in der Zeichenmenge von GitHub-Namen; `reason` als Enum mit allen
  dokumentierten Gründen und `Other`; `subject` mit Titel, Art (`PullRequest`, `Issue`, `CheckSuite`, `Release`,
  `Discussion`, `Commit`, `RepositoryVulnerabilityAlert`, sonst `Other`) und Nummer aus der API-URL
  (`/repos/{o}/{r}/(issues|pulls)/{n}`); `updated_at`, `unread`. `Thread::web_url()` baut den Link nur aus diesen
  Teilen (`/pull/{n}`, `/issues/{n}`, `/actions`, `/releases`, `/discussions`, sonst das Repo), nie mit dem Host aus
  der Antwort (Freigabe 15). `result()` nur aus eindeutigen Titeln („… failed“, „… succeeded“, „… cancelled“), sonst
  `None`.
- **Erledigen:** `done(id)` → `DELETE /notifications/threads/{id}` (204); `read(id)` → `PATCH` (205), für später.
  Der Token nur im `Authorization`-Kopf; kein `Debug`, keine Fehlermeldung trägt ihn; GitHubs Worte nur über eine
  feste Meldung (`public_message`).
- **API/MCP:** keine.
- **Abnahme:** Fixture-Test je Grund und Art; URL-Bau; `subject.url` mit fremdem Host wird nicht übernommen; falsche
  Repo-Namen und Thread-Ids verworfen; `httpmock`: Seiten, 304 mit `If-Modified-Since`, `X-Poll-Interval`, 401, 403
  (Fine-grained), 429 mit `Retry-After`, DELETE 204; Token nicht in `Debug`.

### E5.4 GitHub als Quelle im Kern
- [x] erledigt (2026-10-14: `sources/github.rs` mit einer Schleife für alle Konten (`Poller`, alle 10 s fällige Konten, Takt `X-Poll-Interval`/60 s, `Last-Modified` und `first-poll` in `cursors`); jede Abfrage reicht nur bis 3 Tage vor der ersten des Kontos (auch nach einer Änderung), alte Threads bleiben draußen; 401 → `signed_out`, `check_all` macht es zum Kontoproblem, Pause bis zur neuen Anmeldung; 403 → `missing_scope`, `offline`; stumm → keine Abfrage; Konto weg → `retire_remote` + `forget_source`; `settle_done` schickt `DELETE` nur beim Übergang offen → erledigt (`Settled`, im Speicher); `GithubAccounts::notifications`; `task_from_entry` mit `github:<owner>/<repo>#<n>`. Ohne GitHub-Lesen ist der Eintrag unsichtbar, also `not-found` statt `forbidden` (Freigabe 7); Lesen ohne Schreiben `forbidden`)
- **Modell:** Opus · **Braucht:** E5.2, E5.3, G.1 · **Dateien:** `crates/torroworkplace/src/sources/github.rs`,
  `crates/torroworkplace/src/account.rs` (nur `GithubAccounts::notifications(id)`), `crates/torroworkplace/src/core.rs`
  (nur `task_from_entry`: Kennung `github:`); Doku: `docs/{api,eingang}.md`
- **Abfrage je Konto:** Takt = größer von `X-Poll-Interval` und 60 s, mit `Last-Modified` aus `cursors`; die erste
  Abfrage nur Threads der letzten 3 Tage (`since`); stumme Quelle → keine Abfrage; entferntes Konto beendet die Schleife
  (`retire_remote`); 401 → Pause bis zur neuen Anmeldung, `health: signed_out` (Kontoproblem wie bisher aus
  `check_all`); `NeedsScope` → `health: missing_scope`, keine Einträge.
- **Eintrag:** Quelle `github:<login>`, Vorgang = Thread-Id, Kennung der Quelle = Thread-Id; Titel = Titel des Subjects
  (fremd), kein Text (die App baut die Metazeile aus den Merkmalen); Link `web_url()`; Merkmale nach E5.1; Art und
  Spur aus `github_place`; neues `updated_at` = neue Aktivität (Zähler +1, Erledigtes kommt wieder).
- **Erledigt sich selbst:** Fehlt ein Thread in einer vollständigen Liste, wurde er anderswo gelesen →
  `resolve_remote`; eine unvollständige Liste löst nichts auf.
- **Zurückmelden:** `settle_done` → `done(id)`; Fehler nur ins Log (Kennung, Code); Undo ändert auf GitHub nichts.
- **Aufgabe:** bei Issue und PR `link` = `github:<owner>/<repo>#<n>` (G.6-Verweis); ohne GitHub-Lesen → `forbidden`,
  Repo außerhalb → `not-found` (wie `link_refusal`); sonst `inbox:<id>` mit der Web-Seite als `url`.
- **Oberfläche:** Einträge im Benachrichtigungen-Fenster und in der Zentrale; Schalter und Hinweis auf der Kontoseite
  in E5.14. **API:** vorhandene Befehle, `inbox.list_sources` mit `health`. **MCP:** vorhandene Tools.
- **Abnahme:** Tests mit `httpmock` als GitHub und fester Uhr: Eintrag mit Merkmalen und Spur; neue Aktivität zählt;
  304 → nichts; anderswo gelesen → erledigt; unvollständige Liste → nichts erledigt; Erledigen schickt genau ein
  DELETE, Undo keins; stumm → keine Abfrage; Konto entfernt → Einträge erledigt und Statistik weg; erste Abfrage nur 3
  Tage; `X-Poll-Interval` eingehalten; Aufgabe mit `github:`-Kennung samt Rechten; Protokoll ohne Titel und Repo.
  `torro-api-review`.

### E5.5 GitHub: Einträge erledigen sich selbst
- [x] erledigt (2026-10-14: `sources/github.rs` prüft nach einer Abfrage, die GitHub beantwortet hat, höchstens alle
  5 Minuten die offenen und zurückgestellten Einträge mit Merkmal `kind` `issue`/`pull_request` (Nummer aus dem Link);
  neu `Api::standing` mit `graphql::{standing_body, read_standing, Standing}`: dasselbe Dokument wie `Api::items` (50
  je Aufruf) plus `closedAt` und `reviewRequests` (`User.isViewer`; ein angefragtes Team zählt als „noch angefragt“),
  `NOT_FOUND`/`FORBIDDEN` = außer Reichweite, bleibt. Geschlossen/gemergt → erledigt, aber erst 2 Minuten nach dem
  Schließen (GitHubs eigene Meldung dazu kommt vorher an und öffnet den Eintrag nicht wieder) und nur, wenn die letzte
  Aktivität höchstens 5 Minuten nach dem Schließen liegt (spätere Kommentare sind Neues); `review_requested` und nicht
  mehr angefragt → erledigt. Erledigt wie in der App (`inbox.set_item_done`), die Kennungen bleiben, also bringt die
  unveränderte Liste nichts zurück; kein `settle_done`, kein DELETE)
- **Modell:** Opus · **Braucht:** E5.4 · **Dateien:** `crates/torroworkplace/src/sources/github.rs`,
  `crates/torro-pim-github/src/{graphql,api}.rs` (nur wenn `Api::items` ein Feld fehlt, z. B. ob der Angemeldete noch
  als Reviewer angefragt ist; nennen), Fixtures
- Szene 4 in `eingang.md`: PR gemergt oder Review abgegeben → der Eintrag erledigt sich. Offene Einträge zu Issues und
  PRs höchstens alle 5 Minuten über `Api::items` (50 je Aufruf); gemergt oder geschlossen → erledigt;
  `review_requested` und nicht mehr angefragt → erledigt; Repos außerhalb der Reichweite des Tokens unberührt. Nichts
  wirkt auf GitHub zurück.
- **API/MCP:** keine.
- **Abnahme:** Fixtures für gemergt, geschlossen, Review abgegeben, offen (bleibt); kein DELETE; höchstens ein Aufruf
  je 50 Einträge und Runde.

### E5.6 GitHub-Mail dem GitHub-Eintrag zuordnen
- [x] erledigt (2026-10-14: `MessageTraits.github` = `X-GitHub-Reason` (nur `a–z` und `_`), im Mail-Speicher Schema 4
  (`github_reason`); `NewMail.github: Option<IssueRef>` aus `inbox_feed::github_thread` nur, wenn `From`, `List-Id` und
  `Message-ID` passen – nur die `Message-ID`, `In-Reply-To` wird nicht gespeichert (jede GitHub-Mail nennt ihr Issue
  selbst, `docs/mail.md`); `inbox.sqlite` Schema 9 (`mail_messages.github`): mit offenem oder zurückgestelltem
  GitHub-Eintrag derselben Nummer einer nicht stummen Quelle wird die Mail kein Eintrag und zählt nicht, sonst ein
  Mail-Eintrag, der beim Kommen (oder Wiederöffnen) des GitHub-Eintrags in ihn aufgeht (erledigt, nicht als erledigt
  gezählt); ein erledigter GitHub-Eintrag nimmt keine Mail; `Inbox::mail_by_account`; `check_done` verlangt bei
  angehängter Mail Schreiben in `mail` und jedes Konto; `mark_read` setzt `\Seen` auch für GitHub-Einträge. Offen: die
  Freigabe-Zusammenfassung (`approvals.rs`) nennt die Mail noch nicht)
- **Modell:** Opus · **Braucht:** E5.4, E4.1, Freigabe 12 · **Dateien:** `crates/torro-mail/src/message.rs`,
  `crates/torro-mail-imap/src/mime.rs` (+ Fixture einer GitHub-Mail), `crates/torroworkplace/src/mail/inbox_feed.rs`,
  `crates/torroworkplace/src/inbox.rs`, `crates/torroworkplace/src/sources/github.rs`; Doku: `docs/{eingang,mail}.md`
- **Erkennen:** eine GitHub-Mail nur, wenn alle drei passen: `From: notifications@github.com` und `X-GitHub-Reason`;
  `List-Id: <repo.owner.github.com>` → owner/repo; Message-ID bzw. `In-Reply-To`
  `<owner/repo/(pull|issues)/<n>…@github.com>` → Nummer. Sonst eine gewöhnliche Mail.
- **Zuordnen** (nur mit verbundener, nicht stummer GitHub-Quelle): Mail zuerst → abgelegt und gemerkt
  (`gh:owner/repo#n`); kommt der GitHub-Eintrag desselben Vorgangs, ist der Mail-Eintrag erledigt (als zusammengeführt
  gezählt) und seine Nachrichten hängen am GitHub-Eintrag. GitHub zuerst → die Mail wird kein Eintrag, hängt am
  GitHub-Eintrag und zählt nicht doppelt. Ohne GitHub-Quelle bleibt die Mail der Weg.
- **Zurückmelden:** Erledigt am GitHub-Eintrag macht den Thread „done“ **und** markiert die zugehörigen Mails gelesen
  (für Clients zusätzlich Schreiben in `mail` und das Konto, `check_mail_done`).
- **Fälschung:** Kopfzeilen kommen von außen; schlimmstenfalls hängt eine Mail an einem vorhandenen GitHub-Eintrag
  desselben Repos und derselben Nummer. Ohne solchen Eintrag verschwindet nie eine Mail. Test.
- **API/MCP:** keine neuen; die Beschreibung von `inbox.set_item_done` nennt die Mail.
- **Abnahme:** Fixture `review_requested`; beide Reihenfolgen; widersprüchliche Kopfzeilen → gewöhnliche Mail; ohne
  GitHub-Quelle → gewöhnlich; Erledigt → DELETE und `\Seen`; Client ohne Mail-Schreiben → `forbidden`; Protokoll ohne
  Betreff.

### E5.7 Nextcloud: Benachrichtigungen und Talk lesen (Crate `torro-nextcloud`)
- [x] erledigt (2026-10-14: `Nextcloud` mit `capabilities`, `notifications(etag)`, `delete_notification`, `rooms`,
  `mark_read`, `rich::{plain,line}`; Weiterleitungen nur im selben Origin; 404 beim Löschen gilt als erledigt)
- **Modell:** Opus · **Braucht:** – (sofort möglich nach Freigabe 10) · **Dateien:** neu `crates/torro-nextcloud/`
  (`Cargo.toml`, `src/{lib,ocs,capabilities,notifications,talk,rich}.rs`, `tests/fixtures/`), Wurzel-`Cargo.toml`
- **OCS:** Hülle entpacken; `OCS-APIRequest: true`, `Accept: application/json`; Basic Auth nur im Kopf; keine
  Weiterleitung auf einen anderen Origin. Fehler: `Refused` (401), `Forbidden` (403), `Maintenance` (503), `Throttled`
  (429/Brute-Force-Drossel mit `Retry-After`), `MissingApp` (404), `Offline`, `Other`. Endpunkte vorher in der
  `openapi.json` der App nachlesen und im Code verlinken (Skill `nextcloud`).
- **Capabilities:** ob `notifications` (mit `list`/`delete`) und `spreed` (Feature `chat-read-marker`) da sind, dazu
  die `notify_push`-Adresse (nur lesen).
- **Benachrichtigungen:** `GET /ocs/v2.php/apps/notifications/api/v2/notifications` mit ETag/`If-None-Match` →
  `NotModified` oder `List`; Id (Ziffern), App, `object_type`, `object_id`, Zeit, Betreff und Text als Klartext aus
  `subjectRich`/`messageRich`, `link` (nur gleicher Origin), Person (Parameter der Art `user`); `delete(id)`.
- **Talk:** `GET /ocs/v2.php/apps/spreed/api/v4/room`: Token (geprüfte Zeichen), Art, Name, `unreadMessages`,
  `unreadMention`, `unreadMentionDirect`, `notificationLevel`, letzte Nachricht (Id, Person, Klartext),
  `lastActivity`; `mark_read(token, message_id)` als `POST /ocs/v2.php/apps/spreed/api/v1/chat/{token}/read`.
- **`rich::plain`:** ersetzt `{key}` durch den `name` des Parameters, entfernt Steuer- und Richtungszeichen, Titel eine
  Zeile.
- **API/MCP:** keine.
- **Abnahme:** Fixtures (anonymisiert) für Benachrichtigungen, Talk, Capabilities; `httpmock`: 200, 304, 401, 404 (App
  fehlt), 503, DELETE, `read`; Link mit fremdem Origin verworfen; Platzhalter ersetzt; App-Passwort nie in `Debug`.

### E5.8 Nextcloud-Benachrichtigungen als Quelle
- [x] erledigt (2026-10-14: `PimService::{nextcloud_accounts, nextcloud_access}` mit `NextcloudAccess` (Client und `sign_in` = SHA-256 des App-Passworts, damit 401 bis zur neuen Anmeldung pausiert); je Konto eine Schleife, Lesestand `etag` und `since` (erste Abfrage 3 Tage, beim Stummschalten auf jetzt); Nextcloud liefert höchstens die 25 neuesten, darum streicht eine volle Liste nur Ids über ihrer ältesten; `wanted` als Stelle für E5.9; `#[allow(dead_code)]` der Ablage in `inbox.rs` entfernt)
- **Modell:** Opus · **Braucht:** E5.2, E5.7 · **Dateien:** `crates/torroworkplace/src/sources/nextcloud.rs`,
  `crates/torroworkplace/src/pim.rs` (nur `PimService::nextcloud_access(id)` und die Liste der Nextcloud-Konten, wie
  `GithubAccounts::api`), `crates/torroworkplace/Cargo.toml`; Doku: `docs/{api,eingang}.md`
- **Je Konto der Art Nextcloud** (nicht CalDAV, nicht Mail): Capabilities beim Start und stündlich (App fehlt →
  `health: missing_app`); Abfrage alle 60 s mit ETag, die erste nur die letzten 3 Tage; 401/403 → Pause und
  `signed_out` (Fernlöschung prüft der Kalender-Abgleich); 503 → warten; stumm → keine Abfrage.
- **Eintrag:** Quelle `nextcloud:<login>@<host>`, Vorgang `<app>:<object_type>:<object_id>` (zu lang: Prüfsumme),
  Kennungen der Quelle = Ids der Benachrichtigungen; Titel und Text als Klartext; Link nur mit gleichem Origin;
  Merkmale `app`, `person`; Spur aus `nextcloud_place`. Aktionen von Nextcloud (Freigabe annehmen) nicht in E5.
- **Erledigt sich selbst:** Id auf dem Server weg → gestrichen; ohne Id → erledigt. **Zurückmelden:** löscht alle
  Benachrichtigungen des Eintrags.
- **Oberfläche:** E5.14, E5.15. **API/MCP:** vorhandene.
- **Abnahme:** `httpmock` als Nextcloud: Eintrag mit Merkmalen; zwei Benachrichtigungen desselben Gegenstands → ein
  Eintrag mit Zähler; auf dem Server gelöscht → erledigt; Erledigen löscht alle Ids; 304 → nichts; App fehlt →
  `health`; kein Link mit fremdem Origin; Protokoll ohne Texte.

### E5.9 Talk: ungelesene Unterhaltungen und Erwähnungen
- [x] erledigt (2026-10-14: Talk in der Schleife von E5.8 (`Poller` mit `talk::Talk`, gemeinsame Anmeldung und Capabilities; ohne `spreed` keine Quelle und kein `health`); Zähler = ungelesene Nachrichten über eine Kennung je neuer Nachricht (`<token>/<id>`, `…~<n>`, höchstens 99 je Runde), wie viele neu sind, aus dem Lesestand `room:<token>`; Erledigen sendet `read` mit der neuesten Nachrichten-Id und löscht die Chat-Benachrichtigungen bis zur letzten Meldung; `wanted` lässt `spreed`/`chat` weg, solange der Server Talk hat; nach Stummschalten zählt erst Neues (nur im Speicher); Katalogtexte von `set_item_done` und `create_task_from_item` nennen das Löschen der Chat-Benachrichtigungen; `clients.rs` unverändert, `talk:` streicht schon E5.2)
- **Modell:** Opus · **Braucht:** E5.8 · **Dateien:** `crates/torroworkplace/src/sources/{talk,nextcloud}.rs`,
  `crates/torroworkplace/src/clients.rs` (nur falls E5.2 `talk:` beim Entfernen noch nicht streicht); Doku:
  `docs/{api,eingang}.md`
- **Quelle `talk:<login>@<host>`** je Nextcloud-Konto mit `spreed`; Räume alle 60 s. Ein Eintrag je Unterhaltung,
  soweit `talk_place` es sagt: Art `message`, Vorgang = Raum-Token, Zähler = ungelesene Nachrichten, Titel = Name der
  Unterhaltung (fremd), Text = „Anna: …“ aus der letzten Nachricht, `from` die Person, Link `<server>/call/<token>`,
  Merkmale `person`, `kind`.
- **Keine Doppelten:** Solange Talk läuft, legt E5.8 Chat-Benachrichtigungen von `spreed` nicht mehr ab (Anrufe
  bleiben).
- **Erledigt sich selbst:** ungelesen = 0. **Zurückmelden:** gelesen bis zur letzten bekannten Nachricht, dazu die
  passenden `spreed`-Benachrichtigungen löschen; Neueres bleibt ungelesen und öffnet den Eintrag wieder.
- **Antworten** noch nicht (offene Frage 8); Knöpfe wie „Chat“ ohne Reply.
- **Oberfläche:** eigene Spalte „Talk“ (E5.15). **API/MCP:** vorhandene.
- **Abnahme:** jede Benachrichtigungsstufe je Raumart; anderswo gelesen → erledigt; Erledigen sendet `read` mit der
  richtigen Nachrichten-Id; keine Doppelten mit E5.8; Talk getrennt freigebbar (ein Client mit `nextcloud:` sieht
  `talk:` nicht); Datenregel: entferntes Modell nur Id.

### E5.10 Push über `notify_push` (optional)
- [ ] erledigt
- **Modell:** Opus · **Braucht:** E5.8, Freigabe 11 · **Dateien:** neu `crates/torro-nextcloud/src/push.rs`,
  `crates/torro-nextcloud/Cargo.toml`, Wurzel-`Cargo.toml` (TLS-Feature), `crates/torroworkplace/src/sources/{nextcloud,talk}.rs`
- **Verbindung:** `wss://`-Adresse aus den Capabilities **nur** mit gleichem Origin wie der Server des Kontos, `ws://`
  nur zu diesem Rechner; sonst nie verbinden, nie das Passwort senden. Anmelden mit Loginname und App-Passwort (zwei
  Textnachrichten), Antwort `authenticated`.
- **Ablauf:** `notify_notification` löst sofort eine Abfrage von Benachrichtigungen und Räumen aus; solange Push
  steht, nur noch alle 5 Minuten abfragen; Wiederverbinden mit wachsender Pause; Fehler → normale Abfrage.
- **API/MCP:** keine.
- **Abnahme:** Fake-WebSocket-Server (lokal im Test): Push löst die Abfrage aus; fremder Origin → keine Verbindung und
  kein Passwort; Anmeldefehler → weiter abfragen.

### E5.11 ntfy: Protokoll (Crate `torro-ntfy`)
- [x] erledigt (2026-10-14: `Server`, `Topic`, `Login`, `Ntfy::{health,check,subscribe}`, `Subscription::next`, `read_event`; Fixtures nach der ntfy-Doku ohne Netz, gegen einen echten Server ungeprüft)
- **Modell:** Opus · **Braucht:** – (sofort möglich nach Freigabe 10) · **Dateien:** neu `crates/torro-ntfy/`
  (`Cargo.toml`, `src/{lib,server,subscribe,message}.rs`, `tests/fixtures/*.ndjson`), Wurzel-`Cargo.toml`
- **Server:** Adresse vereinheitlichen (ohne Schema `https`; leer = `https://ntfy.sh`); `may_send_login`: Zugangsdaten
  nur über `https` oder zu diesem Rechner (wie `key_may_travel`); keine Weiterleitungen. **Themen:** nur
  `[A-Za-z0-9_-]{1,64}`.
- **Anfragen:** Abo `<server>/<t1>,<t2>/json?since=<id>`; Anmeldung `Bearer tk_…` (Token) oder Basic; `health` =
  `GET /v1/health`; `check` = `?poll=1&since=latest` (401/403 → `auth`).
- **Lesen:** Zeilen aus `Response::chunk()`, höchstens 64 KiB je Zeile (längere verworfen); Ereignisse `open`,
  `keepalive`, `message`, Unbekanntes ignoriert; Nachricht: Id, Zeit, Thema, Titel, Text, Priorität (1–5, Standard 3),
  Tags (höchstens 10 zu je 64 Zeichen), `click` (nur `http`/`https`); `actions`, `attachment`, `icon` nie abgerufen.
- **API/MCP:** keine.
- **Abnahme:** Fixtures; Zeilen über Chunk-Grenzen; zu lange Zeile verworfen; `httpmock` für `health`, `check`, 401;
  Login über `http` zu einem anderen Rechner → abgelehnt; `Secret` nicht in `Debug`.

### E5.12 ntfy-Konto: Gerüst, Requests, Katalog
- [x] erledigt (2026-10-14: `AccountKind::Ntfy`, `Account::ntfy`, `NtfyConfig`/`NtfyTopic` und `torro_ipc::TopicName` (Name nie im `Debug`); `account/ntfy.rs` mit `NtfyAccounts::{connect, set_login, set_topic, remove, infos(names), label_of, changes}` und `NtfyAccountError`; Etikett Pflicht (eine Zeile, höchstens 60 Zeichen), höchstens 50 Themen; `set_ntfy_login` ergänzt bei einem vorhandenen Konto die Themen, statt sie zu ersetzen; `set_ntfy_topic` antwortet `ok`; `AccountInfo.ntfy` (`NtfyView`, Themen-Namen nur für den App-Schlüssel); Freigabe nennt Server und Etikett (`Facts::topic`); Haken für E5.13 ist `NtfyAccounts::changes` – `Sources` bekommt die ntfy-Konten erst in E5.13, weil `sources/{github,nextcloud}.rs` `Sources` in Tests wörtlich bauen)
- **Modell:** Opus · **Braucht:** E5.2, E5.11, Freigabe 13, 14, 16 · **Dateien:** `crates/torro-core/src/account.rs`,
  neu `crates/torroworkplace/src/account/ntfy.rs` (`NtfyAccounts`, wie `AiAccounts`),
  `crates/torroworkplace/src/{account,core,mcp,approvals,audit,daemon}.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torroworkplace/src/assistant/model.rs`; Doku: `docs/{api,eingang,settings}.md`
- **Requests** (Bereich `accounts`):

  | Request | Zugriff | MCP | Bemerkung |
  |---|---|---|---|
  | `Request::ConnectNtfy { server, topics }` · `accounts.connect_ntfy` | Admin | `accounts_connect_ntfy` | App-Schlüssel: `health`, dann `check` der Themen ohne Login, dann speichern; `auth`, wenn ein Thema einen Login braucht. Client: zeigt die Steuerzentrale (Accounts), antwortet `opened`, speichert nichts (wie `connect_github`) |
  | `Request::SetNtfyLogin { server, topics, user?, secret }` · `accounts.set_ntfy_login` | Admin | **Never** | Nur App-Schlüssel („Was nie nach außen geht“ 5); verbindet oder ersetzt den Login; Geheimnis nur im Schlüsselbund, nie in Antwort, Freigabe, Protokoll oder `Debug` |
  | `Request::SetNtfyTopic { account, topic, label?, subscribed }` · `accounts.set_ntfy_topic` | Admin | `accounts_set_ntfy_topic` | Thema anlegen, umbenennen, abbestellen; Freigabe nennt Server und Etikett |

- **Fehlercodes:** `not-found`, `auth`, `offline`, `error` (falsche Adresse oder Thema, Login über `http` an einen
  anderen Rechner), `conflict` (gleicher Server zweimal).
- **Ansicht:** `AccountInfo.ntfy` mit Server, Themen (Etikett; Themen-Namen nur für den App-Schlüssel, offene Frage
  10), Benutzer, „Login gespeichert“.
- **Entfernen:** `accounts.remove_account` löscht für ntfy das Geheimnis und erledigt die Einträge (`retire_remote`,
  `forget_source`); das Abo endet (Haken für E5.13). **Protokoll:** nur die Konto-Id, nie Themen. **Assistent:**
  Bereiche nach Freigabe 16.
- **Oberfläche:** E5.14. **API/MCP:** wie oben, `set_ntfy_login` Never.
- **Abnahme:** Katalog-Tabellentest; `names_follow_the_rules` mit `accounts.set_ntfy_login` in `kept_back`; Client:
  `set_ntfy_login` → `forbidden`, `connect_ntfy` → `opened`; Login über `http` an einen anderen Rechner → `error`;
  Geheimnis nirgends in Antwort, `Debug`, Protokoll, `approvals.json`; MCP-Tools bauen die Requests; `list_accounts`
  je Schlüssel; `torro-api-review` ohne hohen Befund.

### E5.13 ntfy: Themen abonnieren
- [x] erledigt (2026-10-14: `sources/ntfy.rs` mit einer Schleife je Konto (`Subscriber::session`, Ende `Gone`/`Renew`/`Retry`/`Paused`/`Muted`), Pause 1 s doppelnd bis 5 min, nach 1 min stabiler Verbindung wieder 1 s; Lesestand `since` = letzte Id, bei der ersten Verbindung die Unix-Zeit ihres `open`, beim Stummschalten gelöscht; neues Thema → neu ab jetzt, neues Etikett hält die Verbindung; 401/403 und Login ohne TLS → `signed_out`, `offline`; Vorgang `<16 hex SHA-256(Salz + Thema)>:<24 hex SHA-256(Titel)>` mit zufälligem Salz der Quelle in `cursors` (`thread_key` geht an Clients und entfernte Modelle); Drossel im Speicher, „More from {topic}“ je Thema; nach dem Entfernen wird nichts mehr abgelegt; `NtfyAccounts::{ids, access}` mit `NtfyAccess`/`AccessTopic`; `Sources.ntfy`, verdrahtet in `Core::sources`)
- **Modell:** Opus · **Braucht:** E5.12 · **Dateien:** `crates/torroworkplace/src/sources/ntfy.rs`,
  `crates/torroworkplace/src/account/ntfy.rs` (Zugang für das Abo); Doku: `docs/eingang.md`
- **Abo je Konto:** eine Verbindung für alle Themen; Wiederverbinden mit wachsender Pause (1 s bis 5 min) und
  `since=<letzte Id>` aus `cursors`; die erste Verbindung holt nichts nach; geändertes Thema → neu verbinden; stumm oder
  Konto entfernt → trennen; ohne Daten (auch ohne `keepalive`) länger als 2 min → neu verbinden.
- **Eintrag:** Quelle `ntfy:<host>`, Vorgang `<thema>:<titel>` (gekürzt bzw. Prüfsumme); Art und Spur aus
  `ntfy_place` (Priorität 5 → `alert`, dringend, auch in Ruhe); Titel = Titel oder erste Zeile, Text = Nachricht;
  Merkmale `topic` (Etikett), `priority`, `tag`; Link `click`; immer `untrusted`.
- **Drossel:** mehr als 60 neue Vorgänge je Stunde und Konto landen gesammelt in „More from {topic}“ (Zur Info), bis
  die Stunde um ist. **Zurückmelden:** keins.
- **API/MCP:** vorhandene.
- **Abnahme:** `httpmock` mit NDJSON-Körper: Nachricht → Eintrag; gleicher Titel → Zähler; Priorität 5 → dringend,
  Popup auch in Ruhe (Test mit `inbox_notify`); Priorität 2 → Info; neu verbinden sendet `since`; `keepalive`
  ignoriert; Drossel; stumm trennt; 401 → `health: signed_out`; Protokoll ohne Titel und Themen.

### E5.14 Steuerzentrale: ntfy einrichten, Eingang je Konto
- [x] erledigt (2026-10-14: Assistent „ntfy“ Server → Themen (Name und Etikett je Zeile, Fehler erst nach dem ersten
  „Connect“, Prüfung mit den Sätzen des Kerns) → bei `auth` Anmeldung (Benutzer und Passwort oder Token, `Secret`, beim
  Senden genommen) → „Your topics are subscribed.“; Kontoseite ntfy mit Themen (Dialog zum Umbenennen, „Unsubscribe…“
  mit Rückfrage, „Add topic…“), „Change login…“ und „Remove account…“; Gruppe „Inbox“ bei GitHub (ein Schalter,
  `missing_scope` als Satz) und Nextcloud („Notifications“, „Talk“, Grund unter dem Namen), je `inbox.mute_source`;
  Agents & API bietet GitHub, Nextcloud, Talk und ntfy je Konto an, GitHub mit dem Hinweis; Demo `--open
  wizard-ntfy|wizard-ntfy-topics|wizard-ntfy-invalid|wizard-ntfy-login|wizard-ntfy-done`, Demo-Konto
  `ntfy:push.example.org` und Quellen; die Kontoseite ntfy und ihre Dialoge haben noch keinen `--open`-Zustand, weil
  `ui/mod.rs` dafür einen Zweig braucht; deutsche Texte trägt die Koordination ein)
- **Modell:** Opus · **Braucht:** E5.4, E5.8, E5.9, E5.12 · **Dateien:**
  `crates/torroworkplace/src/ui/{wizard,accounts,agents,launch}.rs`, `crates/torro-core/src/i18n/de.rs`
- **Assistent „ntfy“** (`Provider::Ntfy`): Server („ntfy.sh when empty“) → Themen (je Zeile Thema und Etikett) →
  Login falls nötig (Benutzer und Passwort oder Token, Feld als `Secret` wie G.1b) → Prüfen → „Your topics are
  subscribed.“
- **Kontoseite ntfy:** Themen hinzufügen, umbenennen, abbestellen; „Change login…“; „Remove account…“.
- **Kontoseite GitHub:** Gruppe „Inbox“ mit dem Schalter „Show notifications in the Inbox“ (= `inbox.mute_source`);
  bei `missing_scope` der Satz „Notifications need a classic token with notifications or repo, or the GitHub CLI
  sign-in.“
- **Kontoseite Nextcloud:** Schalter „Notifications“ und „Talk“ (je `mute_source`); bei `missing_app` ein ruhiger Satz.
- **Agents & API:** Quellen-Auswahl mit „GitHub“, „Nextcloud“, „Talk“, „ntfy“; bei GitHub der Hinweis „Also needs read
  access to GitHub and the repository“.
- **API:** `accounts.connect_ntfy`, `set_ntfy_login`, `set_ntfy_topic`, `remove_account`, `inbox.mute_source`,
  `inbox.list_sources`. **MCP:** vorhandene Tools.
- **Abnahme:** Tests der Nachrichten, `Debug` ohne Geheimnis; Sichtprüfung `torroworkplace ui --demo --open …` in Breit
  und Schmal, hell, dunkel, Plex.

### E5.15 App: Icons, Spalten und Knöpfe der neuen Quellen
- [x] erledigt (2026-10-14: `Icon::{MessageCircle, RadioTower}`; `list::source_icon` nur nach der Art (Skript `github`
  → `AppWindow`), `list::{source_name, facet_words, facet_line, waiting_word}`: Konto im Namen nur bei zwei Konten
  einer Art (Spaltenköpfe, Karten, „This window shows“, Chips, Seitenleiste), Metazeile aus den Merkmalen in Karten,
  To sort und als zweite Zeile ohne Text, Merkmale als Eigenschaften im Paneel; Alarm „Urgent“ und Kachel in
  `attention` getönt; `hub::Origin::Service(SourceKind)`, Insights `Group::Source(Origin)`; Knöpfe schon wie in der
  Tabelle (Tests je Art); Demo `demo-13` aus GitHub, ntfy mit Link; Galerie zeigt die Quellen-Icons; Sichtprüfung auf
  macOS (AeroSpace, nur das eigene Fenster schwebend) `--view notifications|hub` in sechs Größen, beiden Dichten, hell,
  dunkel, Plex; ntfy weiter nur aus der Demo, E5.13 offen)
- **Modell:** Opus · **Braucht:** E5.1; Sichtprüfung mit echten Daten nach E5.4, E5.8, E5.13 · **Dateien:**
  `crates/torro-gui/src/icon.rs`, neu `crates/torro-gui/assets/icons/{message-circle,radio-tower}.svg` (Lucide 0.469.0,
  Kopf und Strichstärke wie E8.10), `crates/torro-gui/examples/gallery.rs`,
  `crates/torroinbox/src/{list,notifications,hub,insights,detail,app}.rs`, `crates/torro-inbox/src/demo.rs`,
  `.claude/skills/torro-design/SKILL.md`, `crates/torro-core/src/i18n/de.rs`
- **`list::source_icon`:** GitHub `Github`, Nextcloud `Cloud`, Talk `MessageCircle`, ntfy `RadioTower`. Der Kniff aus
  E8.13 („Skript namens `github`“) entfällt; `demo-13` kommt aus einer echten GitHub-Quelle.
- **Metazeile** aus den Merkmalen mit `tr()` („owner/repo · Review requested“, „Files · Anna“, „Priority 5“); fremde
  Werte als Klartext.
- **Knöpfe** nach der Tabelle in `eingang.md`: GitHub, Nextcloud, ntfy ✕ · Open · Task · Later · Glocke; Talk ✕ · Open
  · Task · Later · Glocke ohne Reply; `alert` wie Dringendes hervorgehoben, ohne Torro-Rot.
- **Spalten** je Quelle, bei zwei Konten derselben Art mit Konto im Namen; Kacheln der Zentrale je Quellenart
  (`hub::Origin`). **Insights:** Reihen Mail, Agenten, dann die zwei meistgenutzten anderen Quellen, dann „Other“.
- **API/MCP:** keine.
- **Abnahme:** Tests: Icon je Art (ein Skript namens `github` zeigt kein GitHub-Icon mehr); Spalten GitHub, Nextcloud,
  Talk, ntfy im Demo; Knopfliste je Art; Reihen von Insights; `scripts/design-check.sh`; Galerie mit den neuen Icons;
  Sichtprüfung `--view notifications|hub --demo` in allen Größen, beiden Dichten, hell, dunkel, Plex.

### Fragen zu E5, entschieden am 2026-10-14 (alle nach Empfehlung)

1. **GitHub „Erledigt“:** Thread als *done* löschen (`DELETE`) oder nur *gelesen* (`PATCH`)? Empfehlung: `DELETE` für
   Erledigt und ✕; Öffnen ändert auf GitHub nichts.
2. **Spuren je GitHub-Grund:** Tabelle in E5.1, besonders `author` unter „für mich“? Empfehlung: ja; `comment` und
   `subscribed` unter Info.
3. **„CI rot auf eigenem PR → dringend“:** Prüfläufe zeigen nicht verlässlich auf den PR, das Ergebnis steht nur im
   Titel. Empfehlung: in E5 nichts Dringendes von GitHub, fehlgeschlagene Prüfläufe „für mich“, dringend erst in E6.
4. **Voreinstellung für vorhandene Konten:** Eingang für GitHub und Nextcloud an oder aus? Empfehlung: an, erste
   Abfrage nur 3 Tage, Ausschalten auf der Kontoseite (= Quelle stumm).
5. **Fine-grained GitHub-Tokens** lesen keine Benachrichtigungen. Empfehlung: kein Kontoproblem, nur der Hinweis über
   `health: missing_scope`.
6. **Talk als eigene Quelle `talk:`** statt Merkmal von Nextcloud? Empfehlung: eigene Quelle (eigene Spalte, Knöpfe,
   getrennt freigebbar, Lesestand je Raum).
7. **Gruppenchats ohne Erwähnung:** Empfehlung: die Benachrichtigungsstufe von Talk entscheidet; „immer“ = Info,
   Standard = nur Erwähnungen.
8. **Antworten in Talk in E5?** Empfehlung: nein; später als allgemeines `inbox.reply` mit „Nach außen“ und Freigabe
   (mit Slack, E7).
9. **ntfy-Priorität 5 als dringend** (Ergänzung zu Sicherheitsregel 3)? Empfehlung: ja, nur Priorität 5, nur in
   abonnierten Themen, mit Drossel.
10. **ntfy-Themen** sind auf öffentlichen Servern praktisch Passwörter. Empfehlung: Themen-Namen sieht nur der
    App-Schlüssel; Clients und das Merkmal `topic` sehen das Etikett; nie ins Protokoll.
11. **Vorgang bei ntfy:** Empfehlung: Thema plus Titel, damit wiederholte Meldungen („Backup failed“) zusammenfallen.
12. **Mehrwertige Merkmale (Tags):** `nth` in `MuteScope::Facet` oder nur der erste Tag? Empfehlung: `nth`.
13. **Icons:** Lucide (`cloud`, `message-circle`, `radio-tower`, `github`) oder Marken-Logos? Empfehlung: Lucide;
    Logos später nach Klärung von Lizenz und Design-Regel.
14. **Neue Crates** `torro-nextcloud` und `torro-ntfy` oder Module im Kern? Empfehlung: Crates wie `torro-pim-github`.
15. **Datenregel:** Nextcloud und Talk für entfernte Modelle zurückhalten, GitHub und ntfy durchlassen? Empfehlung: ja,
    bis KI.1 Regeln je Quelle bringt.
16. **GitHub-Mail:** Erledigt am GitHub-Eintrag markiert die zugeordnete Mail gelesen? Empfehlung: ja.
17. **`notify_push` in E5?** Empfehlung: als letzte, optionale Aufgabe (E5.10); Abfrage alle 60 s mit ETag reicht.


## Danach (Überblick, Aufgaben folgen)

- **E4 Mail als Quelle**, nach dem Mail-Kern (M2/M3 in [mail.md](../mail.md)); dort entscheiden: Spur
  „Belege“, „erledigt“ archiviert oder nicht.
- ~~**E5 Nextcloud, GitHub, ntfy**~~: geplant, siehe [E5](#e5-nextcloud-github-ntfy-als-quellen-plan-2026-10-14). Danach dazu: „CI rot am eigenen PR“ als dringend (E6), Antworten in Talk über `inbox.reply` (mit E7), Aktionen von Nextcloud, Gotify.
- **E6 Ruhe und Ordnung:** Screener, Lernen, Zusammenführen über Quellen; mit [ki.md](../ki.md) Spur
  vorschlagen und Zusammenfassungen.
- **E7 Slack**, danach Messenger (zweiter Schritt).
- **An einen Agenten geben** (vorgemerkt 2026-10-13): Ablauf, Rechte, Rückweg des Ergebnisses und Aussehen werden
  eigens ausgearbeitet, bevor es Aufgaben gibt ([eingang.md › Später](../eingang.md#später-vorgemerkt)).
- **Fokus-Profile** (vorgemerkt 2026-10-13): Profile wie der Fokus bei Apple, im Anschluss an E8.6 (gleiche Regeln)
  und E3.4 (Ruhe); Entwurf in Reihe 4 der Zeichenfläche.
