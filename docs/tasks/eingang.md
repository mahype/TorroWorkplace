# Aufgaben: Eingang (Inbox), Schritte E1–E3

Konzept: [eingang.md](../eingang.md). Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill
`torro-task`. Wer einen Request anfasst, lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`.

E1–E3 bringen den Eingang **ohne Mail und ohne fremde Anmeldung**: Kern, Meldungen der Suite,
Agenten (Claude Code, Codex, Gemini CLI) und die App `torroinbox`. Mail kommt mit E4 (Konzept
[mail.md](../mail.md)), KI mit [ki.md](../ki.md). Die Kategorien eingehender Mail (E12) haben
ein eigenes Konzept: [kategorien.md](../kategorien.md).

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

### E8.14 Uhrzeit statt „17 h“ (Issue #115)
- [x] erledigt (2026-10-14: `list::stamp` ersetzt `list::ago` in Zeilen und Karten aller Fensterarten, im Mail-Fenster
  und im Blick; heute „16:40“, gestern „Yesterday 16:40“, sonst das kurze Datum; das Paneel behält „4 min ago“ neben
  Datum und Uhrzeit; `torrotasks` bleibt bei „Synced 2 min ago“, das ist ein Stand, kein Eingang)
- **Modell:** Opus · **Braucht:** E8.13 · **Dateien:** `crates/torroinbox/src/{list,glance,mail}.rs`,
  `crates/torro-core/src/i18n/de.rs`
- Ein Eintrag oder eine Mail von heute zeigt die Uhrzeit, nicht wie lange es her ist; relative Angaben muss man
  zurückrechnen. Gestern mit Uhrzeit, damit „16:40“ nicht für heute gehalten wird; ältere mit kurzem Datum. Die
  Zeitgruppen „Today/Yesterday“ bleiben.
- **Oberfläche:** Zeilen und Karten (Eingang, Benachrichtigungen, Zentrale, Mail), Blick. **API/MCP:** keine (nur
  Darstellung).
- **Abnahme:** Tests für `stamp` (vor 5 min, vor 23 h am selben Tag und am Vortag, letzte Woche, Uhr vorgehend), in
  beiden Sprachen.

### E8.15 Nach Eingang sortieren, auf- und absteigend (Issue #116)
- [x] erledigt (2026-10-14: `torro_mail::Order { NewestFirst, OldestFirst }`, wieder ausgeführt als
  `torro_inbox::Order` und `torro_ipc::Order`; `order` in `mail.search_messages` (`SearchQuery`, SQL, beide Teile der
  Suche: älteste zuerst erst die Treffer auf dem Server, dann die des Speichers, Cursor je Reihenfolge) und in
  `inbox.list_items` (`ItemFilter`, `rules::order_in`, SQL, Demo); Dringendes bleibt oben, nur innerhalb der Gruppen
  gedreht, die Tagesgruppen der Übersicht drehen mit; Knopf „Newest first“/„Oldest first“ in der Kopfleiste
  (Lucide `arrow-down-wide-narrow` / `arrow-up-narrow-wide`, Wort wie beim Dichte-Umschalter), gemerkt je Fenster in
  `ui.json` (`ui_state::order_from`, Schlüssel wie die Dichte); nicht auf „Act now“ und Insights der Zentrale und
  nicht in „Invitations“; Blick (Mini, Leiste) bleibt neueste zuerst)
- **Modell:** Opus · **Braucht:** E8.14 · **Dateien:** `crates/torro-mail/src/{lib,order}.rs`,
  `crates/torro-inbox/src/{lib,source,rules,demo}.rs`, `crates/torro-ipc/src/{lib,api,inbox,mail}.rs`,
  `crates/torroworkplace/src/{inbox,mcp}.rs`, `crates/torroworkplace/src/mail/{read,store}.rs` (und `order: None` in
  Tests und internen Requests), `crates/torro-gui/src/{icon,ui_state}.rs`, zwei Icons,
  `crates/torroinbox/src/{app,mail,notifications,overview}.rs`, `crates/torro-core/src/i18n/de.rs`
- Nur im Client umdrehen geht nicht: „Load more“ (Cursor) und `limit` folgen der Reihenfolge.
- **Oberfläche:** Kopfleiste jeder Liste im Mail-, Benachrichtigungs- und Zentrale-Fenster. **API:**
  `Request::SearchMail.order` · `mail.search_messages` · Read; `Request::ListItems.order` · `inbox.list_items` · Read
  (neues optionales Feld, verträglich). **MCP:** `mail_search_messages` und `inbox_list_items` (Eingabe `order`).
- **Abnahme:** Tests: Reihenfolge und Cursor in beiden Richtungen im Mail-Speicher, in der Suche über Speicher und
  Server, in `inbox.list_items` (SQL) und `ItemFilter::apply`; MCP baut `order` mit; `ui.json` je Fenster; App dreht
  Ordner, „Load more“ und Eintragslisten und verwirft Seiten der alten Reihenfolge.

### E8.16 Mail fehlt unter „For me“: Grund am Eintrag (Issue #128)
- [x] erledigt (2026-10-14: Ursache war `List-Unsubscribe` allein – die Mail eines Creators an Unterstützer kam ohne
  `List-Id`, `Precedence` und `Auto-Submitted`, galt als Massenmail und lag in „Info“, bis sie anderswo gelesen und
  der Eintrag erledigt war. `inbox_feed::bulk_reason` ersetzt `is_bulk`: `List-Unsubscribe` allein zählt nicht mehr;
  `NewMail.bulk` trägt den Grund. Neu `Item.reason` (`LaneReason`: `list`, `bulk`, `automated`, `sender`, `muted`),
  vom Kern beim Ablegen gesetzt, bei Verschieben von Hand gelöscht; Paneel und Leser zeigen „Info · Mailing list
  (List-Id)“. Fixture `tests/fixtures/mail/creator-post.eml`)
- **Modell:** Opus · **Braucht:** E4.1 · **Dateien:** `crates/torro-inbox/src/{item,rules,demo,lib}.rs`,
  `crates/torroworkplace/src/{inbox,mail/inbox_feed}.rs` (Literale in weiteren Dateien, die der Compiler erzwingt),
  `crates/torroinbox/src/{detail,mail}.rs`, `crates/torro-core/src/i18n/de.rs`, `docs/eingang.md`
- **Oberfläche:** Zeile „Lane“ im Paneel und eine leise Zeile unter dem Absender im Leser des Mail-Fensters.
  **API:** neues optionales Feld `reason` an `Item` in `inbox.get_item`/`inbox.list_items` (verträglich, kein neuer
  Befehl). **MCP:** `inbox_get_item`, `inbox_list_items` liefern es mit.
- **Abnahme:** Fixture mit den Kopfzeilen der Mail landet „for me“ (rein und über den Feed); Grund für Liste,
  gemerkten Absender, Stummschaltung; von Hand verschoben ohne Grund; beide Sprachen.

### E8.17 Verwerfen heißt weg (Issue #117)
- [x] erledigt (2026-10-14: ✕ „Dismiss“ (`x`) zuletzt in der Aktionsleiste der Eintragszeilen (`list::actions`) für
  alles, was `notifications::dismissable` ist – „Act now“ und das Benachrichtigungsfenster hatten es schon; Hinweis
  „Dismissed“ mit Undo wie bisher. Die Ansicht „Done“ (auch „Done today“ der Zentrale) fragt mit `dismissed: false`
  und zeigt Verworfenes nicht mehr. Der Kern löscht die Marke `dismissed`, sobald ein Eintrag nicht mehr erledigt ist
  (von Hand oder durch eine neue Meldung wieder offen), damit der Filter nie einen offenen Eintrag verbirgt.
  Entscheidung: Mail wird weiter nicht verworfen – „Done“ tut bei Mail genau das (gelesen, nicht verschoben), ein
  zweiter Knopf mit gleicher Wirkung wäre doppelt; das Paneel bleibt ohne ✕, `x` wirkt dort schon)
- **Modell:** Opus · **Braucht:** E8.4, E8.6a · **Dateien:** `crates/torro-ipc/src/{lib,api,inbox}.rs`,
  `crates/torro-inbox/src/{source,demo}.rs`, `crates/torroworkplace/src/{inbox,mcp}.rs` (und `dismissed: None` in
  Tests und internen Requests), `crates/torroinbox/src/{app,list}.rs`
- **Oberfläche:** Aktionsleiste der Zeilen (Zentrale: Quellen, Later; Benachrichtigungen), Ansicht „Done“. **API:**
  `Request::ListItems.dismissed` · `inbox.list_items` · Read (neues optionales Feld, verträglich); Verwerfen bleibt
  `inbox.set_item_done` mit `dismissed: true`. **MCP:** `inbox_list_items` (Eingabe `dismissed`),
  `inbox_set_item_done`.
- **Abnahme:** Tests: verworfener Eintrag fehlt in offenen Einträgen und in „Done“ (Kern-SQL, Demo, App), kommt mit
  neuer Meldung offen zurück; ✕ in der Zeilenleiste sendet `Dismiss`, Mail hat keins; MCP baut `dismissed` mit.

### E8.18 Archivieren und Löschen aus der Liste, Antworten nur im Leser (Issue #118)
- [x] erledigt (2026-10-14: `mail::filing_actions` baut Archive (`e` im Ordner) und Delete (`#`, neu auch Entf und
  ⌫) als `Message::Mail` – dieselben Wege wie Tasten und Leser, mit Hinweis und Undo. Ordner: Karten Archive · Delete ·
  Task, kompakte Zeilen bekommen dieselbe Aktionsleiste auf der ersten Zeile. Mail-Einträge (For me, Info, Later):
  Archive · Delete vor Task · Done · Later · „…“ in Zeile und Karte; Archive ohne eigene Taste, `e` bleibt dort Done.
  Reply fehlt in Zeilen und Karten und steht nur im Leser; Mail-Karten haben damit keine Hauptaktion mehr (alle Wörter
  oder keins). Entscheidung: Ohne Archiv- bzw. Papierkorbordner im Konto fehlt der Knopf, statt gesperrt zu sein – so
  will es die Regel der Aktionsleiste („nur Aktionen, die gehen“), und `RowAction` kennt keinen gesperrten Knopf;
  ebenso kein Archive im Archiv, kein Delete im Papierkorb. Einladungen behalten ihre Antworten · Later · Task)
- **Modell:** Opus · **Braucht:** E4.2, E8.3, E8.11 · **Dateien:** `crates/torroinbox/src/{mail,list,keys,app}.rs`
  (Feld `folders` in `list::Rows` und `mail::MailList`, auch in Tests von `hub`, `notifications`, `overview`)
- **Oberfläche:** Zeilen und Karten im Mail-Fenster (Ordner, smarte Ansichten, Einträge), Zeilen der Mail-Einträge in
  der Zentrale. **API:** vorhanden, `mail.move_messages` (Rolle Archive) · Write und `mail.delete_messages` · Delete;
  kein neuer Request. **MCP:** `mail_move_messages`, `mail_delete_messages`.
- **Abnahme:** Tests: Knöpfe senden Archive/Delete für die richtige Nachricht, kein Reply in Zeilen und Karten, Knopf
  fehlt ohne Ordner der Rolle; Entf/⌫ löschen; Löschen aus der Ordnerzeile mit Undo. Sichtprüfung kompakt/bequem.

### E8.19 Menüs am angeklickten Knopf (Issue #122)
- [x] erledigt (2026-10-14: Die Fenster führen zwei Sätze `detail::Anchors` – Knöpfe von Paneel/Leser und die Zeile
  bzw. Karte unter dem Cursor samt ihren Knöpfen (`Anchors::mark`/`wrap`, neu `RowAction::anchor`/`at` und
  `widgets::anchored_if` in `torro-gui`). `App::menu_at` merkt, woher das Menü kam: `Act` aus Zeile/Karte → deren
  Knopf, `Ask…` aus Paneel/Leser → dessen Knopf, Taste → Paneel, solange es offen ist, sonst Zeile. Fehlt der Knopf,
  fällt der Anker auf den Knopf der Zeile, dann die Zeile zurück (`Anchor::or`); ohne jeden Anker steht das Menü
  mittig im Fenster statt oben links (`widgets::floating`). Entscheidungen: „Task“ der Ordnerzeile sendet jetzt
  `Act(…, MakeTask)` wie `t`, die Glocke im Paneel des Benachrichtigungsfensters `AskHide` wie „…“ – so ist die Quelle
  des Klicks eindeutig. Sichtprüfung nur in Breit (macOS ohne Hyprland, Fenstergröße nicht setzbar); die Ansichten aller
  Größen rendern im Test)
- **Modell:** Opus · **Braucht:** E3.9, E8.12 · **Dateien:** `crates/torro-gui/src/widgets.rs`,
  `crates/torroinbox/src/{app,detail,list,mail,overview,hub,notifications}.rs`
- **Oberfläche:** Later, Hide/„…“, Move to, Task, Event, Glocke (Mute) in Zeilen, Karten, Paneel und Leser.
  **API/MCP:** keine – Fensterzustand, kein Request.
- **Abnahme:** Tests: Klick in der Zeile → Zeilenknopf auch bei offenem Leser, Klick im Leser → Leserknopf, Taste → Leser
  bzw. Zeile; Tasten im offenen Menü verschieben es nicht; Knopf ↔ Anker; Rückfall-Kette und Mitte in `torro-gui`.

### E8.20 Mail-Liste ohne Leser über die ganze Breite (Issue #129)
- [x] erledigt (2026-10-14: `App::list_cap` nimmt dem Mail-Fenster die Lesebreite 720: „For me“ und die Ordner füllen
  in Mittel und Schmal die ganze Inhaltsfläche, in Breit ihre Spalte neben dem Leser; die Karten rechnen mit derselben
  Breite (`App::card_width`). Zentrale und Benachrichtigungen behalten ihre Lesebreite. Entscheidung: keine eigenen
  Spalten (Absender | Betreff | Zeit) für sehr breite Listen – das Mail-Fenster zeigt in Breit immer die Spalte des
  Lesers, eine Liste ohne Leser gibt es dort nicht; bleibt bei Bedarf eine eigene Aufgabe)
- **Modell:** Opus · **Braucht:** E8.11 · **Dateien:** `crates/torroinbox/src/app.rs`
- **Oberfläche:** Liste des Mail-Fensters (kompakt und bequem). **API/MCP:** keine – Darstellung.
- **Abnahme:** Test: keine Begrenzung im Mail-Fenster, Karten so breit wie die Fläche, andere Fenster begrenzt.

### E8.21 Ziehbare Trennlinie zwischen Liste und Leser (Issue #130)
- [x] erledigt (2026-10-14: neuer Baustein `torro_gui::split` – Griff `space::s()` breit mit der Linie in der Mitte,
  Zeiger „resize“; Drücken startet, während des Ziehens meldet die ganze Teilung die Zeigerposition, Loslassen irgendwo
  im Fenster beendet (`split::release`), Doppelklick = Standardbreite `size::list()`. Beide Seiten behalten mindestens
  `split::min_pane()` = `size::props()` (300). Zustand `split::Split` mit `update`, das nur bei echter Änderung
  „speichern“ meldet; `ui.json` unter `split.<Fensterschlüssel>` (`ui_state::save_split_task`, Zurücksetzen löscht den
  Eintrag). Das Mail-Fenster nutzt ihn in Breit; die Karten rechnen mit der gezogenen Breite. Sichtprüfung: gemerkte
  Breite 640 im Bild; Ziehen selbst nicht automatisiert geprüft (keine Eingabe ans Fenster möglich). Befund nebenbei:
  Die Knopfleiste des Lesers läuft unter etwa 450 Breite rechts über (Close, Move to, „…“ abgeschnitten) – schon vorher
  bei 1100 breitem Fenster, eigenes Issue)
- **Modell:** Opus · **Braucht:** E8.3, E8.20 · **Dateien:** `crates/torro-gui/src/{split,ui_state,lib}.rs`,
  `crates/torroinbox/src/app.rs`
- **Oberfläche:** Mail-Fenster, Breit: Trennlinie zwischen Liste und Leser. **API/MCP:** keine – Fensterzustand in
  `ui.json`, kein Request.
- **Abnahme:** Tests: Breite begrenzt (beide Mindestbreiten), gespeichert nur bei Änderung, Zurücksetzen, `ui.json`
  je Fenster neben den anderen Schlüsseln; App: Ziehen, Karten folgen, gemerkte Breite verliert gegen Ziehen.

### E8.22 Quellen und Konten in der Seitenleiste eingeklappt (Issue #121)
- [x] erledigt (2026-10-14: „Sources“ hat einen Umschalter (`Message::ToggleSources`), die Kontogruppen des
  Mail-Fensters starten zu – offen ist ohne Wahl nur das Konto des gezeigten Ordners. Aufgeklappt merkt `ui.json`
  (`unfolded`) `sidebar.sources` und `sidebar.mail.<Konto>`; zugeklappt fällt der Name heraus. Beim Speichern bleiben
  Namen, die das Fenster nicht entscheidet (andere Fenster des Eingangs, spätere Versionen), erhalten. Dazu die
  Übersetzung des Knopfs „Archive“: neu `i18n::trc(context, english)` mit Tabelleneintrag `"action|Archive"` →
  „Archivieren“ für Knöpfe (Zeile, Karte, Leser, Benachrichtigung), der Ordner bleibt `tr("Archive")` → „Archiv“.
  Entscheidung: Kontext statt anderem englischem Wort, weil Ordner und Knopf englisch gleich heißen)
- **Modell:** Opus · **Braucht:** E3.11, E4.2 · **Dateien:** `crates/torroinbox/src/{app,mail,list}.rs`,
  `crates/torro-core/src/i18n/{mod,de}.rs`, `crates/torroworkplace/src/inbox_notify.rs`
- **Oberfläche:** Seitenleiste (Eingang: „Sources“, Mail-Fenster: Konten), Knöpfe „Archivieren“. **API/MCP:** keine –
  Fensterzustand in `ui.json`, kein Request.
- **Abnahme:** Tests: neuer Start → zu; Aufklappen → gespeichert → nächster Start offen; Zuklappen vergisst; fremde
  Namen bleiben; `trc` mit und ohne Eintrag, beide Sprachen.

### E8.23 Text markieren und kopieren, Knopfleiste des Lesers bricht um (Issue #119)
- [x] erledigt (2026-10-14: neuer Baustein `torro_gui::selectable` – `Selectable` ist ein nur lesbarer `text_editor`
  (Änderungen werden verworfen), der nur die Tasten des Lesens nimmt: ⌘C/Strg+C, ⌘A/Strg+A, Umschalt+Pfeile, Escape;
  alle anderen Tasten (`e`, `j`, Enter …) bleiben beim Fenster. Links sind darin nicht klickbar, darum stehen die
  Web-Adressen des Textes darunter als „Links“ (`selectable::links_section`, Zeilen wie im Kalender, Schnitt aus
  `torro_gui::links`; `mailto:` und alles andere öffnet nicht). Der Eingang hält einen `Selectable` für den Text in
  Leser bzw. Paneel (`App::follow_text`, neu nur, wenn sich der Text ändert). „Copy text“ (`Icon::Copy`): im Leser
  bei den Werkzeugen, im Paneel rechts neben den Hinweisen unter der Mail bzw. neben dem Titel „Message“ eines
  Eintrags; Hinweis „Copied“. Die Eigenbau-Erkennung `mail::pieces` entfällt. Entscheidungen: der Text steht so, wie
  er geschrieben ist (keine geschützten Bindestriche mehr, `detail::unbroken` entfällt), damit Kopiertes stimmt;
  die Knopfleiste des Lesers geht, wenn die Icons nicht in eine Zeile passen, in zwei Zeilen (Werkzeuge rechts in
  der zweiten, beide brechen in sehr schmalen Fenstern um) statt Aktionen in „…“ zu verstecken. Der Kalender nutzt
  den Baustein noch nicht (eigene Kopie in `editor.rs`), eigene Aufgabe)
- **Modell:** Opus · **Braucht:** E8.3, E8.21 · **Dateien:** `crates/torro-gui/src/{selectable,lib}.rs`,
  `crates/torroinbox/src/{app,mail,detail}.rs`, `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** Leser des Mail-Fensters, Paneel eines Eintrags bzw. einer Mail. **API/MCP:** keine – der Text kommt
  schon über `inbox.get_item` / `mail.get_message`, Markieren und Kopieren sind reine Oberfläche.
- **Abnahme:** Tests: Änderungen ändern den Text nie, nur Lesetasten werden genommen, „Copy text“ schreibt einmal in
  die Zwischenablage mit dem Klartext und zeigt „Copied“, Links ohne `mailto:`/`file:`, Knopfleiste in zwei Zeilen
  unter der Breite einer Icon-Zeile. Sichtprüfung: Markieren mit Maus und ⌘C.

### E8.24 KI-Aufgabe und KI-Termin aus Eintrag oder Mail (Issue #120)
- [x] erledigt (2026-10-14: zwei neue Requests `inbox.suggest_task_from_item` / `inbox.suggest_event_from_item`
  (Bereich Inbox, Zugriff Write wegen KI-Kosten, MCP-Tools gleichen Namens, offene Welt). Der Kern liest den Eintrag
  wie `inbox.get_item` (bei Mail den ganzen Text aus `mail.get_message`, sonst den Eintragstext), wählt die KI
  (`suggest::choose`: Standard des Assistenten aus `assistant_default`, wenn das ein KI-Konto ist, sonst das erste
  KI-Konto ohne Problem; Coding-Agenten zählen nicht) und fragt sie einmal ohne Werkzeuge nach genau einem
  JSON-Objekt. Der Inhalt steht als Daten zwischen `<message>`-Marken (eine schließende Marke im Text wird
  aufgebrochen), die Anweisung sagt, nichts darin zu befolgen. Datenregel: Einträge von Mail, Nextcloud und Talk gehen
  nur an lokale Modelle (`data_rule::held_back`); dann wird ein lokales Konto statt des Standards genommen, ohne
  eines `not-connected`. Daten liest der Kern in der Zeitzone der Person (`2026-10-16` → Tag,
  `2026-10-16T14:30` → Zeitpunkt; ganztägiges Ende ist der letzte Tag). Gespeichert wird nichts; dafür bekommen
  `inbox.create_task_from_item` ein optionales `notes` und `inbox.create_event_from_item` optionale `location` und
  `notes` (vor den Eintragstext gesetzt, Grenzen 2000/200 Zeichen, Freigabe-Zusammenfassung nennt sie). App:
  „AI task“ / „AI event“ (`Sparkles`) neben „Make task“ / „Make event“ im Paneel und nach „Task“ im Leser; ohne KI
  aus, Tooltip „AI task · Set up an AI first“ (`assistant.list_engines` beim Start und je Takt). Der Knopf öffnet
  den vorhandenen Dialog sofort, eine leise Zeile sagt „Asking your AI…“, dann füllt der Vorschlag Titel,
  Fälligkeit (`Due::On` für andere Tage), Notiz bzw. Tag, Beginn, Dauer (`Length::Minutes`), Ort und Teilnehmer
  („With: …“ als Notiz). Entscheidungen: Knöpfe nur in Paneel und Leser – dort, wo der Inhalt gelesen wurde, wie
  „Reply“; Zeilen und Karten behalten ihre Leisten (`RowAction` kennt keinen ausgegrauten Zustand; nachgeholt in E8.27). Ganztägige
  Vorschläge werden 09:00 für eine Stunde, weil der Dialog nur Termine mit Uhrzeit anlegt. Teilnehmer landen in der
  Notiz, weil ein Termin der Suite keine Teilnehmer hat. Kein Tastenkürzel)
- **Modell:** Opus · **Braucht:** E3.5, E8.5b, C.15 · **Dateien:** `crates/torro-ipc/src/{lib,api,inbox}.rs`,
  `crates/torroworkplace/src/{suggest,core,inbox,approvals,audit,daemon,mcp,pim,main}.rs`,
  `crates/torroworkplace/src/assistant/{mod,data_rule}.rs`, `crates/torroinbox/src/{suggest,app,detail,mail,main}.rs`,
  `crates/torroinbox/Cargo.toml`, `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** Paneel eines Eintrags, Leser des Mail-Fensters; Dialoge „Make task“ / „Make event“.
- **API:** `Request::SuggestTaskFromItem` · `inbox.suggest_task_from_item` · Zugriff Write;
  `Request::SuggestEventFromItem` · `inbox.suggest_event_from_item` · Zugriff Write.
- **MCP:** `inbox_suggest_task_from_item`, `inbox_suggest_event_from_item`.
- **Abnahme:** Tests mit Fake-Provider: Datum im Text wird `due` bzw. Terminzeit (`suggest.rs`, Kern-Test mit
  Ollama-/OpenRouter-Attrappe samt Datenregel). Ohne KI sind die Knöpfe gesperrt und zeigen den Tooltip (`tr(...)`).

### E8.25 Stummschalten im Mail-Fenster, ein Menü für Mail (Issue #123)
- [x] erledigt (2026-10-14: Entscheidung: „Hide from inbox“ und „Mute“ sind für Mail **ein** Menü, die Glocke
  „Mute…“ (`App::mutes_by_bell`, in jedem Fenster). Sie steht in Zeilen und Karten statt „…“
  (`list::hide_action`, `mail::message_buttons`), im Leser und im Paneel; `m` öffnet sie. Dauer neu mit „1 week“
  (`MuteFor::OneWeek`): 1 Stunde · bis morgen (= heute) · 1 Woche · immer. „This conversation“ + „Hide“ + „Always“
  ist das alte „This conversation“, Absender/Domain sind Merkmale, „Always to Info“ entfällt (Absender + „Only
  Info“), „Move to junk“ bleibt als letzter Punkt. Andere Quellen behalten „Hide from inbox“ (This entry, Mute
  source). Ordnernachrichten ohne Eintrag: Glocke in Zeile, Karte, Leser und Paneel (`Message::AskMuteMail`),
  `inbox.list_mute_options`/`inbox.mute_item` nehmen die Kennung der Mail (`m:…`): mit Eintrag gilt der Eintrag
  (`Inbox::entry_of_message`), ohne werden nur die Merkmale aus dem Mail-Speicher angeboten
  (`rules::message_stand_in`, `inbox_feed::message_stand_in`, `Inbox::handle_message`); die Unterhaltung ist dort
  `error`. Freigaben nennen Absender usw. aus dem Speicher. Entfernt: `Message::HideSender`, `Undo::Hidden` im
  Fenster (der Request `inbox.hide_sender` bleibt für API/MCP). Demo-Zustand `--open mute-mail`. Sichtprüfung nicht
  möglich: macOS ohne Hyprland, die Ansichten aller Größen rendern im Test)
- **Modell:** Opus · **Braucht:** E8.6b, E8.19 · **Dateien:** `crates/torroinbox/src/{app,mail,list,detail,keys}.rs`,
  `crates/torro-inbox/src/{rules,demo}.rs`, `crates/torroworkplace/src/{core,inbox}.rs`,
  `crates/torroworkplace/src/mail/inbox_feed.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torro-core/src/i18n/de.rs`, `docs/{api,eingang}.md`
- **Oberfläche:** Mail-Fenster (Zeilen, Karten, Leser, Paneel; auch Mail-Einträge der Zentrale). **API:** vorhanden
  `Request::ListMuteOptions` · `inbox.list_mute_options` · Read und `Request::MuteItem` · `inbox.mute_item` · Write
  (Regel Admin); neu: `id` darf eine Mail-Kennung `m:…` sein (kein neuer Befehl, Katalogbeschreibung ergänzt).
  **MCP:** `inbox_list_mute_options`, `inbox_mute_item` (dieselben Tools).
- **Abnahme:** Tests: Mute im Mail-Fenster sendet `inbox.mute_item` (Unterhaltung, Domain, Absender, 1 Woche);
  eine Unterhaltung für eine Weile hält weitere Mails aus „For me“ (Kern); Ordnernachricht ohne Eintrag nur mit
  Merkmalen, für Clients nur mit Konto, Freigabe nennt den Absender aus dem Speicher.

### E8.26 Fenster unterscheidbar: Notification Hub, Mail, App Feed (Issue #127)
- [x] erledigt (2026-10-14: Entscheidung 21 in [eingang.md](../eingang.md). Zentrale „Notification Hub“
  (de „Mitteilungszentrale“), Mail-Fenster „Mail“, Benachrichtigungen-Fenster „App Feed“ (de „App-Feed“), weil
  „Notification Hub“ und „Notifications“ sich ähnelten. Lange Namen „Torro Notification Hub“, „Torro Workplace
  Mail“ (nicht „Torro Mail“: TorroMail ist ein eigenes Produkt), „Torro App Feed“. Umbenannt in Tray-Menü,
  App-Umschalter, Titeln, Steuerzentrale (Abschnitt, Konten, Agenten, Assistent), Freigaben, CLI, Desktop-Hinweisen,
  Assistent und API-Beschreibungen; „Inbox“ heißt nur noch der Mail-Ordner. Erkennungszeichen `shell::app_mark`:
  Icon des Fensters in eigener Farbe (Standardfarben der Listen: Zentrale orange, Mail blau, App Feed pink) auf
  getönter Kachel statt des Logos im Umschalter und in der Leiste, ohne Seitenleiste und Leiste (Schmal) vorn in der
  Kopfleiste. „For me“ im Mail-Fenster mit `UserCheck`. Aktive Seitenleistenzeile bleibt neutral (Design-Regel
  „Auswahl neutral“). Unverändert: Fensterklassen, `AppId`, `--view`, API-Bereich `inbox`)
- **Modell:** Opus · **Braucht:** E8.1 · **Dateien:** `crates/torro-core/src/{app.rs,i18n/de.rs}`,
  `crates/torro-gui/src/shell.rs`, `crates/torroinbox/src/{app,overview,list,detail,mail,keys}.rs`,
  `crates/torroworkplace/src/{tray/*,ui/*,approvals,cli,core,inbox_notify,mcp}.rs`, `crates/torro-ipc/src/api.rs`,
  `crates/torro-inbox/src/source.rs`, `crates/torroassistant/src/{chat,view}.rs`, `docs/{eingang,app-catalog}.md`
- **Oberfläche:** alle drei Fenster, Tray, Steuerzentrale. **API/MCP:** keine neuen Befehle, nur Beschreibungen
  (`apps.open_app`, `inbox.list_items`, `inbox.open_item`, Fehlertexte der Sicherheitsregel 1, MCP-Anleitung).
- **Abnahme:** Tests: die drei Namen teilen kein Wort (beide Sprachen), jedes Fenster eigenes Icon und eigene Farbe,
  Titel „Notification Hub · …“ / „Mail · …“ / „App Feed · …“, „For me“ ohne `Icon::Inbox`.

### E8.27 Gesperrte Icon-Knöpfe gedimmt, KI-Knöpfe an Zeilen und Karten (Issue #168)
- [x] erledigt (2026-10-14: `torro-gui`: Knöpfe ohne Nachricht (`icon_button`, `boxed_button`, `compact_button`,
  `text_button`) blenden ihr Icon auf 40 % ab (`Svg::opacity`, weil Iced beim Tönen des Icons die Deckkraft
  verwirft); `busy_button` bleibt voll. `RowAction` bekommt `blocked(grund)`, `press()` und `tip()`: gesperrt
  steht der Knopf gedimmt in der Leiste, nimmt keinen Klick und nennt den Grund im Tooltip; ohne Taste kein
  `kbd`. Eingang: „AI task“ steht gleich nach „Task“, „AI event“ gleich nach „Event“ – in den Aktionsleisten der
  Zeilen (Zentrale, Mail, App Feed), auf den Karten von Mail-Einträgen, App Feed und Zentrale; ohne KI gesperrt mit
  „Set up an AI first“. Auf Karten sind sie Begleiter (immer nur Icon, zählen nicht beim Wählen zwischen Wörtern
  und Icons), damit die Karten ihre Wörter behalten; in den einzeiligen Karten von „To sort“ treten sie zurück, wo
  die Wörter nur ohne sie passen. Ordnernachrichten ohne Eintrag bekommen keine KI-Knöpfe (der Vorschlag braucht
  einen Eintrag). `suggest::off_button` entfällt)
- **Modell:** Opus · **Braucht:** E8.24 · **Dateien:** `crates/torro-gui/src/widgets.rs`,
  `crates/torroinbox/src/{suggest,list,notifications,hub,overview,mail,app}.rs`
- **Oberfläche:** Icon-Knöpfe überall; Zeilen und Karten des Eingangs. **API/MCP:** keine Änderung
  (`inbox.suggest_task_from_item`, `inbox.suggest_event_from_item` gibt es seit E8.24).
- **Abnahme:** Tests: gesperrte Knöpfe blenden das Icon ab; `RowAction::blocked` ohne Klick, mit Grund; KI-Knöpfe
  an Zeilen und Karten, ohne KI gesperrt mit Tooltip, mit KI schicken sie den Vorschlag; „To sort“ behält die Wörter.

## E9 Mail-Fenster: Posteingang (Plan 2026-10-15, Branch `feat/mail-inbox`)

Die Seitenleiste des Mail-Fensters ist zu unklar: „Übersicht“ und „Info“ zeigen Einträge, nicht Mails, und nur
was der Kern für wichtig hält. Ziel: zwei klare Ansichten oben, wie „Alle Eingänge“ bei Apple Mail.

```
All           ← die Posteingänge aller Konten, eine Liste nach Datum (Startansicht)
Later         ← was der Mensch selbst zurückgestellt hat
Trash         ← die Papierkörbe aller Konten, immer sichtbar
── Smart views: Invitations, With attachments (bleiben), Not delivered (neu, E9.3)
── Konten mit ihren Ordnern (wie bisher, eingeklappt)
```

Entschieden am 2026-10-15:

1. Im Mail-Fenster entfallen „Overview“, „Info“ und „Done“. Die Zentrale und der App Feed bleiben, wie sie sind.
2. **Jede** Mail kann auf später gelegt werden, nicht nur Mails mit Eintrag.
3. Eine Mail auf später **verschwindet aus „All“** und steht nur unter „Later“. Zur gewählten Zeit kommt sie zurück
   und steht dann **oben** in „All“, als wäre sie eben angekommen.
4. Tags über IMAP-Keywords kommen danach, eigener Plan.
5. Der Papierkorb ist immer sichtbar, oben als „Trash“ über alle Konten. Was gelöscht wird, verschwindet sofort aus
   der Liste, in der es stand, auch aus Eintragslisten, und liegt im Papierkorb.
6. Unzustellbare Mails (Bounces) erkennt der Kern selbst und zeigt sie als Smart view „Not delivered“ (E9.3a/b).
7. Der Leser trennt, was jedes Mailprogramm hat, von dem, was nur Torro hat (E9.4): oben eine Kopfleiste mit den
   Mail-Standards als deutliche Knöpfe, unten eine schwebende, fest stehende „Torro-Leiste“ mit Done, Later, Task,
   den KI-Knöpfen und den Ansichts-Schaltern. Archive und Delete bleiben oben. Das Paneel der Zentrale teilt genauso.
8. Neue Mail in einer zurückgestellten Unterhaltung holt sie zurück, auch in der Zentrale.

Reihenfolge: E9.1 (Kern) vor E9.2 (Oberfläche). E9.3a (Kern) nach E9.1, darf neben E9.2 laufen; E9.3b nach E9.2 und
E9.3a; E9.4 (issue #194) zuerst, vor E9.2 und E9.3b, nicht daneben (alle bauen den Leser um): E9.2 und E9.3b setzen
ihre neuen Knöpfe dann gleich in die passende Leiste. Nicht parallel: zwei Aufgaben in
`crates/torroinbox` oder zwei am Katalog.

### E9.1 Später für jede Mail: Kern
- [x] erledigt (2026-10-15): Mail ohne Eintrag wird abgelegt und zurückgestellt, `snoozed` filtert vor dem Blättern,
  Tabelle `woken` (Schema 10) merkt die Rückkehr, `back` sortiert nach oben; Entscheidung 8 über
  `rules::merge_mail` (nur Mail). Gelesene zurückgestellte Mail bleibt in „Later“, bis sie den Posteingang verlässt.
  Festgehalten in [eingang.md › Später für jede Mail](../eingang.md#später-für-jede-mail-e91-entschieden-2026-10-15).
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torroworkplace/src/{core,inbox,mcp}.rs`, `crates/torroworkplace/src/mail/{inbox_feed,service,store,read}.rs`,
  `crates/torro-mail/src/message.rs` (`back`), `crates/torro-inbox/src/*.rs` (nur wenn nötig),
  `docs/{api,api-reference,eingang}.md`; nur zum Bauen (`snoozed: None`, `back: None`):
  `crates/torro-ipc/src/{inbox,mail}.rs`, `crates/torro-mail-imap/src/mime.rs`, `crates/torroworkplace/src/audit.rs`,
  `crates/torroinbox/src/search.rs` (Test)
- **Später ohne Eintrag:** `inbox.snooze_item` nimmt wie `inbox.mute_item` (E8.25) auch die Kennung einer Mail
  (`m:…`). Hat die Mail einen Eintrag (`Inbox::entry_of_message`), gilt der Eintrag. Hat sie keinen, legt der Kern
  einen an (wie der Feed: `Inbox::file_mail`, Spur nach den Regeln, Grund am Eintrag) und stellt ihn zurück. Ohne
  `until` und ohne Eintrag: nichts zu tun, Antwort wie bei einem nicht zurückgestellten Eintrag. Rechte wie
  `inbox.mute_item` mit Mail-Kennung: der Client braucht das Konto (Regel 1), sonst `not-found`.
- **Aus „All“ heraus:** `mail.search_messages` bekommt das optionale Feld `snoozed: Option<bool>`. `false` lässt
  Mails weg, deren Eintrag gerade zurückgestellt ist; `true` liefert nur diese; weggelassen wie bisher. Gefiltert
  wird vor dem Blättern, damit Seiten und `cursor` stimmen (keine halbleeren Seiten).
- **Oben, wenn sie zurückkommt:** Eine Mail, deren Eintrag aus „Later“ zurückkam, sortiert in einer Suche mit
  `snoozed: false` nach dem Zeitpunkt der Rückkehr statt nach ihrem Datum, bis sie gelesen oder erledigt ist.
  `MessageSummary` bekommt dafür optional `back: Option<DateTime<Utc>>` (wann sie zurückkam), damit die Oberfläche
  „Back from later“ zeigen kann. Wie der Kern das ohne Bruch des Cursors löst, entscheidet die Aufgabe; Entscheidung
  in `docs/eingang.md` festhalten.
- **Oberfläche:** Mail-Fenster (E9.2). **API:** vorhanden `Request::SnoozeItem` · `inbox.snooze_item` · Write, neu
  mit Mail-Kennung (Katalogbeschreibung ergänzen); vorhanden `Request::SearchMail` · `mail.search_messages` · Read,
  neues optionales Feld `snoozed`, neues optionales Antwortfeld `back` (verträglich, kein `v: 2`).
  **MCP:** `inbox_snooze_item`, `mail_search_messages` (dieselben Tools, Beschreibung ergänzt).
- **Abnahme:** Tests im Kern: Mail ohne Eintrag auf später → Eintrag zurückgestellt, Mail fehlt in
  `in:inbox` mit `snoozed: false`, steht mit `snoozed: true`; nach Ablauf steht sie mit `back` ganz oben, über
  neueren Mails; Seiten mit `cursor` ohne Lücken und Doppelte; Client ohne das Konto bekommt `not-found`. API-Referenz
  neu erzeugt (`TORRO_BLESS=1 cargo test -p torro-ipc`). `torro-api-review` ohne Befund.

### E9.2 Seitenleiste „All“ und „Later“
- [ ] erledigt
- **Modell:** Opus · **Braucht:** E9.1 · **Dateien:** `crates/torroinbox/src/{app,mail,list,detail,keys,empty}.rs`,
  `crates/torro-core/src/i18n/de.rs`, `docs/eingang.md`
- **Seitenleiste (`App::nav_entries`, `Kind::Mail`):** oben nur „All“ (`Icon::Inbox`, zählt ungelesene Mails aller
  Posteingänge, kein „0“) und „Later“ (`Icon::Clock`). „Overview“, „Info“, „Done“ entfallen im Mail-Fenster. Darunter
  wie bisher „Smart views“ und die Konten.
- **„All“ ist eine neue Ansicht** (`View::AllInboxes`) und die Startansicht des Mail-Fensters (auch für `View::Mail`
  und `--source mail`). Sie lädt `mail.search_messages` ohne Konto, mit `query: "in:inbox"` und `snoozed: false`, eine
  Seite nach der anderen mit `cursor` wie eine Ordneransicht. Zeilen und Karten wie in der Ordneransicht, mit dem
  Konto in der Zeile, wenn es mehr als eins gibt. Mails mit `back` tragen „Back from later“.
- **„Later“ an jeder Mail:** In Zeile, Karte, Leser und Paneel jeder Mail (auch ohne Eintrag, auch in Ordnern) steht
  „Later“ mit der Taste `S` und dem Zeit-Menü wie bei Einträgen; geschickt wird `inbox.snooze_item` mit der
  Mail-Kennung. Danach verschwindet die Mail aus „All“, mit „Undo“ wie bei Einträgen.
- **„Later“-Ansicht:** zeigt die zurückgestellten Mails (Einträge der Quelle Mail) mit der Zeit der Rückkehr;
  „Bring back now“ beendet das Zurückstellen (`inbox.snooze_item` ohne `until`).
- **„Trash“ immer sichtbar:** dritte Zeile oben, „Trash“ (`Icon::Trash`), die Papierkörbe aller Konten in einer Liste
  (`mail.search_messages` ohne Konto, `query: "in:trash"`), wie „All“ mit `cursor`. Kein Zähler.
- **Gelöscht heißt weg (Fehler vom 2026-10-15):** erledigt mit Issue #205 (Mail und Eintrag sofort aus der
  Liste, der Kern schließt den Eintrag nach dem Verschieben aus dem Posteingang). Hier nichts mehr zu tun.
- **Demo und Tastatur:** `--open` Zustände für „All“ (mit zwei Konten, einer Mail „Back from later“) und „Later“;
  Zustände für die entfallenen Ansichten im Mail-Fenster führen auf „All“. Tastenkürzel der Ansichten anpassen
  (`keys.rs`), leere Zustände mit Text („Your inboxes are empty.“, „Nothing for later.“).
- **Oberfläche:** Mail-Fenster. **API/MCP:** keine neuen Befehle, nutzt `mail.search_messages` (Feld `snoozed`) und
  `inbox.snooze_item` mit Mail-Kennung aus E9.1; dieselben MCP-Tools.
- **Abnahme:** Tests: Seitenleiste des Mail-Fensters hat genau „All“, „Later“ und „Trash“ über den Smart views;
  „All“ schickt `mail.search_messages` mit `in:inbox`, ohne Konto, mit `snoozed: false`, und blättert mit `cursor`;
  „Later“ an einer Ordnermail ohne Eintrag schickt `inbox.snooze_item` mit `m:…`; Zentrale und App Feed unverändert. Sichtprüfung
  (`torro-design-review`) in allen Größen, hell und dunkel. Übersetzungen im Bericht.

### E9.3a Unzustellbare Mails erkennen: Kern
- [ ] erledigt
- **Modell:** Opus · **Braucht:** E9.1 · **Dateien:** `crates/torro-mail/src/message.rs`,
  `crates/torro-mail-imap/src/mime.rs`, `crates/torroworkplace/src/mail/{store,service,inbox_feed}.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torroworkplace/src/mcp.rs`, Testdaten unter
  `crates/torro-mail-imap/tests/fixtures/`, `docs/{api,api-reference,eingang}.md`
- **Erkennen:** Eine Mail ist eine Rückmeldung über Nichtzustellung, wenn sie `multipart/report;
  report-type=delivery-status` ist (RFC 3464) und ihr `message/delivery-status`-Teil mindestens einen Empfänger mit
  `Action: failed` (oder `delayed`) nennt. Ersatzweise, für Server ohne DSN: Absender `MAILER-DAEMON@…` oder
  `postmaster@…` mit `Auto-Submitted` oder leerem `Return-Path: <>` und einem Betreff wie „Undelivered“,
  „Delivery Status Notification“, „Mail delivery failed“, „Unzustellbar“. Abwesenheitsnotizen (`auto-replied`) sind
  **keine** Bounces.
- **Was der Kern ausliest:** `MessageSummary.bounce: Option<Bounce>` mit `recipients` (die Adressen, die nicht
  ankamen), `status` (`failed`/`delayed`), `reason` (`Diagnostic-Code`, gekürzt, `untrusted`) und `original` (die
  Message-ID der gesendeten Mail aus `message/rfc822` bzw. `text/rfc822-headers`, wenn vorhanden). Im Index gespeichert,
  beim Neuaufbau des Index auch für alte Mails.
- **Suche:** Operator `is:bounce` (und `-is:bounce`) in `mail.search_messages`.
- **Eingang:** Ein Bounce landet in „For me“, nicht in „Info“ (heute schickt `bulk_reason` maschinelle Mail nach
  „Info“); Grund am Eintrag „Not delivered“.
- **Oberfläche:** Smart view „Not delivered“ (E9.3b). **API:** vorhanden `Request::SearchMail` ·
  `mail.search_messages` · Read, neuer Operator `is:bounce`, neues optionales Antwortfeld `bounce`; `mail.get_message`
  mit demselben Feld. **MCP:** `mail_search_messages`, `mail_get_message` (Beschreibungen ergänzt).
- **Abnahme:** Tests mit Fixtures echter Bounce-Formen (Postfix, Exim, Gmail, Microsoft 365, Dovecot-LDA, eine
  deutsche Meldung, eine Abwesenheitsnotiz als Gegenbeispiel): Empfänger, Status, Grund, Original richtig;
  `is:bounce` findet genau die Bounces; Bounce-Eintrag in „For me“. API-Referenz neu erzeugt.

### E9.3b Smart view „Not delivered“
- [ ] erledigt
- **Modell:** Opus · **Braucht:** E9.2, E9.3a · **Dateien:** `crates/torroinbox/src/{app,mail,list,detail}.rs`,
  `crates/torro-core/src/i18n/de.rs`
- **Seitenleiste:** unter „Smart views“ neben „Invitations“ und „With attachments“: „Not delivered“
  (`Icon::MailWarning` o. ä.), zählt Bounces der letzten 30 Tage, nur sichtbar, wenn es welche gibt. Lädt
  `mail.search_messages` ohne Konto mit `query: "is:bounce"`.
- **Zeile und Leser:** statt des Absenders „Not delivered to {recipient}“, darunter der Grund in einfacher Sprache,
  wo er sich erkennen lässt (Postfach voll, Adresse unbekannt, abgelehnt als Spam), sonst der Text des Servers. Im
  Leser „Open sent mail“ (die Originalmail aus `bounce.original`, wenn sie im Gesendet-Ordner liegt) und „Edit and
  send again“ (öffnet sie als neuen Entwurf).
- **Oberfläche:** Mail-Fenster. **API/MCP:** keine neuen Befehle (E9.3a).
- **Abnahme:** Tests: Smart view erscheint nur mit Bounces und schickt `is:bounce`; Zeile nennt den Empfänger; „Open
  sent mail“ öffnet das Original. Sichtprüfung (`torro-design-review`). Übersetzungen im Bericht.

### E9.4 Leser: Standard oben, Torro-Leiste unten (issue #194)
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torroinbox/src/{mail,detail,app}.rs`,
  `crates/torro-gui/src/{widgets,style}.rs` (schwebende Leiste, größerer Knopf), `crates/torro-core/src/i18n/de.rs`
  (nur wenn neue Texte), `.claude/skills/torro-design/` (Muster „schwebende Leiste“ festhalten)
- **Warum:** Heute stehen im Leser (`mail::reader_actions`) Mail-Standards, Torro-Funktionen und Werkzeuge in einer
  Reihe, alle als kleine `ghost`-Knöpfe (`control_sm`), schmal nur noch Icons. „HTML | Text“ und „Light | Dark“
  (`view_switches`) stehen zwischen Kopf und Mail und scrollen weg. Man sieht nicht, was „normales Mailprogramm“ ist
  und was Torro dazugibt, und die Knöpfe oben wirken nicht wie Knöpfe.
- **Kopfleiste oben (Mail-Standard):** Reply (primär), Reply all, Forward, Trennstrich, Archive, Delete; rechts als
  Icons Flag, gelesen/ungelesen, Close. Die Knöpfe mit Wort sind `secondary` (Fläche und Rand, nicht `ghost`) und
  eine Stufe höher (`size::control()`, 32 statt 28). Sie behalten ihr Wort, solange es passt (wie `bar_fit`), sonst
  Icons mit Tooltip; sehr schmal zwei Reihen wie heute. Die Kopfleiste steht fest über dem Scrollbereich und scrollt
  nicht mit; Betreff, Absender und Mail scrollen darunter.
- **Torro-Leiste unten (was ein Mailprogramm nicht hat):** schwebt über dem Inhalt, fest am unteren Rand des Lesers,
  nicht erst durch Scrollen zu sehen. Eigene Fläche wie ein Menü (Menüfläche, Schatten, `radius::lg`), mittig, mit
  Abstand `space::m()` zu Rand und Seiten. Der Scrollbereich bekommt unten so viel Polster wie die Leiste hoch ist,
  damit das Ende der Mail und die Anhänge nicht verdeckt bleiben. Drei Gruppen mit Trennstrichen:
  1. **Erledigen:** Done bzw. Reopen, Later (Menü), Task (Menü). Bei einer Ordnermail Task (Later kommt mit E9.2
     hier dazu).
  2. **KI:** „AI task“, „AI event“ (`suggest::ai_buttons`), mit `Icon::Sparkles` in Akzentfarbe, damit sie als eigene
     Funktion erkennbar sind; ohne KI aus, mit Tooltip wie heute (issue #120).
  3. **Ansicht:** Move to Info/For me (nur Eintrag), Mute… (Glocke), „HTML | Text“, „Light | Dark“ (nur in der
     HTML-Ansicht), Copy text.
  Knöpfe in derselben Größe und Deutlichkeit wie oben (`secondary`, `control()`), Menüs öffnen nach oben (Anker der
  Leiste). Breit eine Reihe mit Wörtern, mittel zwei Reihen, schmal Icons; was dann nicht passt, geht in „…“. Lädt
  die Mail noch oder ist ein Fehler da: nur die Gruppen, die ohne Inhalt gehen (keine Ansichts-Schalter, kein Copy).
- **Paneel (Zentrale, App Feed, Mail-Fenster schmal):** dieselbe Teilung. Oben im Paneel (`mail::body` → `actions`)
  nur die Mail-Standards, in derselben deutlicheren Form. Die Torro-Funktionen stehen im festen Fuß des Paneels
  (`Panel::footer`): bei einem Eintrag der Fuß aus `detail::Footer` wie heute, dazu die Ansichts-Schalter; eine
  Ordnermail bekommt einen Fuß mit Later, Task, KI, Mute… und den Schaltern. Der Paneel-Fuß bleibt im Stil des
  Paneels (keine schwebende Fläche), Gruppen und Reihenfolge wie im Leser.
- **Tasten bleiben**, die Leiste ändert nur den Ort der Knöpfe (`keys.rs` unverändert, `v` schaltet weiter HTML/Text).
- **Oberfläche:** Mail-Fenster (Leser), Paneel in Zentrale und App Feed. **API/MCP:** keine neuen Befehle; alle
  Knöpfe schicken dieselben Messages und damit dieselben Requests wie bisher (`mail.*`, `inbox.*`,
  `tasks.create_todo_from_mail`, KI-Vorschläge), dieselben MCP-Tools.
- **Abnahme:** Tests: die Kopfleiste hält genau die Mail-Standards (Reply, Reply all, Forward, Archive, Delete, Flag,
  gelesen, Close), die Torro-Leiste genau Done/Reopen, Later, Task, AI task, AI event, Move to, Mute…, die Schalter
  und Copy text, für Eintrag und Ordnermail; die Schalter stehen nicht mehr über der Mail; der Scrollbereich hat
  unten das Polster der Leiste; `bar_fit` für beide Leisten in allen Breiten; Paneel: Standards oben, Rest im Fuß.
  Sichtprüfung (`torro-design-review`) im Leser und im Paneel, alle Größen, hell und dunkel, mit HTML-Mail, Textmail
  und langer Mail (letzte Zeile über der Leiste sichtbar). Übersetzungen im Bericht.

## E11 Mail-Fenster: große Knöpfe (issue #207, Plan 2026-10-16)

Maßstab: Entwurf **D · Kombiniert** in [`docs/prototypes/inbox/mail-buttons.html`](../prototypes/inbox/mail-buttons.html#d)
und der Kommentar „Entscheidung Design (2026-10-16)“ in issue #207. Regeln: Skill `torro-design`. Nur Oberfläche: jeder
Knopf schickt dieselbe Message wie heute und damit denselben Request; keine neuen Requests, keine MCP-Tools, Tasten
(`keys.rs`) unverändert.

Entschieden am 2026-10-16 (Planung #207):

1. **Torro-Knöpfe** (Task, AI task, Event, AI event, Mute… im Leser): runde Pillen mit Rand und Icon in Torro-Rot
   (`Palette::accent`), das Wort in `TEXT`, Fläche neutral. Kein neuer Farb-Token; Ausnahme von „Rot nur
   Primärknopf“ wie bei den KI-Sparkles. Reply ist nicht mehr primär: das Mail-Fenster hat keinen roten Knopf mehr.
2. **Was nicht in der Befehlsleiste steht:** Reply all im Menü an Reply (▾); Flag/Unflag, Mark read/unread,
   Done/Reopen, Move to Info/For me und Close im Menü „…“; AI task und AI event als Pillen neben Task und Event.
3. **Reichweite:** Liste, Leser und Paneel des **Mail-Fensters** in allen Größen. Zentrale und App Feed behalten ihr
   Paneel aus E9.4.
4. **Karten** (Comfortable) bleiben unberührt, bis #208 sie entfernt.
5. „Mute…“ bleibt das Wort (statt „Mute sender“): der Knopf öffnet wie heute das Menü mit allen Wegen, nicht nur den
   Absender.
6. Event steht nur, wo es heute geht (Eintrag mit Mail); eine Ordnermail ohne Eintrag hat keinen Event-Knopf. Later
   an einer Ordnermail kommt mit E9.2.

```
E11.1 ─┬─ E11.2 (Liste)
       └─ E11.3 (Leser) ── E11.4 (Paneel des Mail-Fensters)
```

Nicht parallel: E11.2–E11.4 und E9.2/E9.3b ändern alle `crates/torroinbox`. Wer nach dem anderen kommt, setzt seine
Knöpfe in die neue Anordnung (E9.2: Later an Ordnermails in Befehlsleiste und Leiste der Zeile).

### E11.1 Bausteine: Befehlsleiste, Leiste der Zeile, Pille, versenkter Schalter
- [x] erledigt (2026-10-16): Maße `command`/`pill`, `row_message` 92, `Icon::Code`, Stile `cell`/`pill`/`track`/`text_input_flat`, Bausteine `command_bar` … `file_chip`, `RowAction::accent`, Galerie `--mailbar` (torro und plex, hell und dunkel angesehen)
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-core/src/design.rs`, `crates/torro-core/src/design/rules.rs`,
  `crates/torro-gui/src/{theme,style,widgets,icon}.rs`, `crates/torro-gui/assets/icons/code.svg`,
  `crates/torro-gui/examples/gallery.rs`, `.claude/skills/torro-design/SKILL.md`, `docs/design-pakete.md` (Liste der Maße)
- **Maße** (`torro_core::design::Size`, `Size::entries`, Zuordnung der Schlüssel, `Design::standard()`; in
  `torro_gui::theme::size` je eine Funktion): `command` = 46 (Befehlsleiste des Lesers), `pill` = 42 (Torro-Pillen und
  Avatar im Kopf des Lesers). `size::control_xl()` (44) gilt künftig auch für die Leiste unter der gewählten Mail;
  Doc-Kommentar anpassen. `row_message` 88 → 92 (drei Zeilen 20, oben und unten 16), Doc-Kommentar und Tests
  nachziehen. Grenzen für Pakete in `design/rules.rs`: `command` 32–64, `pill` 28–64 (Tabelle in
  `docs/design-pakete.md` ergänzen). Pakete ohne die neuen Schlüssel nehmen die Standardwerte (Test).
- **Icon:** `Icon::Code` (Lucide 0.469.0 `code`, Strich 1.75) für das Segment „HTML“, mit Eintrag in `Icon::ALL`.
- **Stile** (`style::button`): `cell` – flache Zelle ohne Radius und Rand, Fläche `raised`, Text `text`, Hover `hover`,
  gedrückt `selected`, aus 40 % Deckkraft; `pill` – voll rund, Fläche transparent, Rand `accent`, Text `text`, Hover
  `hover`. `style::container::track` – versenkte Spur: Fläche `bg`, Rand `line_strong`, Radius `radius::sm()`.
  `style::text_input_flat` – ohne Rand und Radius (Suchfeld über der Liste). Alles über `palette(theme)`, keine Zahlen.
- **Bausteine** (`widgets`, je mit Doc-Kommentar und Beispiel):
  - `command_bar(cells)` – Zellen gleich breit (`Fill`) über die ganze Breite, `size::command()` hoch, senkrechte
    `LINE` zwischen den Zellen, unten `LINE`. Zelle `command_cell(icon, label, words, strong, on_press)` – Icon
    `icon::SIZE_SM`, Wort `typo::body()` bzw. mit `strong` `typo::body_strong()`; ohne `words` nur das Icon mit Wort als
    Tooltip. `command_split(icon, label, words, on_press, on_menu)` – wie eine Zelle, rechts ein abgetrennter Teil mit
    `Icon::ChevronDown` (Tooltip „More replies“), der `on_menu` schickt. `command_more(on_press)` – schmale Zelle
    (`size::command()` breit) mit `Icon::Ellipsis`, Tooltip „More“. `command_words(labels, room)` – Wörter nur, wenn
    jede Zelle ihr Icon und Wort in ihrem gleichen Anteil unterbringt, sonst keine.
  - `row_strip(actions, words)` – Leiste unter der gewählten Zeile: die `RowAction`s als `cell`, gleich breit über die
    ganze Zeilenbreite, `size::control_xl()` hoch, oben `LINE`, senkrechte `LINE` dazwischen; Tooltip mit Wort und
    `kbd` wie `row_actions_strip`. `RowAction` bekommt ein Feld `accent` (Icon in `accent`, Methode `.accent()`).
    `row_strip_words(actions, width)` wie `command_words`. `row_actions_strip` bleibt für die Zentrale.
  - `pill_button(icon, label, on_press)` – `size::pill()` hoch, Stil `pill`, Icon in `accent` (aus: verblasst),
    Innenrand waagerecht `space::l()`.
  - `view_switch(options, active)` – versenkte Spur (`container::track`) mit Segmenten aus Icon (ohne Wort, Wort als
    Tooltip), aktives Segment `segment(true)`; Höhe `size::control()`. `file_chip(name, size, on_press)` – 32 hoch
    (`size::control()`), `radius::sm()`, `raised` mit `line_strong`, Büroklammer 14 in `text_3`, Name `typo::body()`,
    Größe `typo::meta()` in `text_3`; ganzer Chip ein Knopf.
- **Galerie:** `--mailbar` zeigt alle neuen Bausteine in allen Zuständen (normal, Hover sichtbar beschriftet, aus),
  Befehlsleiste in drei Breiten, Leiste mit und ohne Wörter. Anschauen mit `--light` und `--design plex`.
- **Skill `torro-design`:** Tabelle der Maße (`command`, `pill`, `control_xl`, `row_message`), Abschnitt 5: neuer Eintrag
  „Befehlsleiste und Leiste der Zeile (E11)“ mit den Bausteinen, der Regel „Aktionen sind Zellen bzw. Pillen, Ansicht
  ist versenkt“ und der Ausnahme für die roten Pillen; der Eintrag „Standard oben, Torro-Leiste unten“ gilt nur noch
  für Zentrale und App Feed.
- **Oberfläche:** nur Bausteine. **API/MCP:** keine.
- **Abnahme:** Tests in `widgets.rs`/`style.rs`: `command_words` und `row_strip_words` an den Grenzen (genau passend,
  ein Pixel zu wenig); `cell` und `pill` in allen fünf Zuständen hell und dunkel, Text ≥ 4.5:1 auf ihrer Fläche,
  Rand der Pille ≥ 3:1 auf `panel`; neue Maße im Standardpaket und in einem Paket ohne sie. Galerie in `torro`, `plex`,
  hell und dunkel angesehen (Bilder im Bericht). `scripts/design-check.sh` sauber. Übersetzungen im Bericht.

### E11.2 Liste: Leiste von Rand zu Rand, Suchfeld über die volle Breite
- [x] erledigt (2026-10-16): `Rows::edge_strip`/`MailList::edge_strip`, `list::{edge_actions, with_row_strip, strip_row, strip_words, edge_strip_height, strip_height_in, row_span_in}`, `search::Bar::full`, `shell::sidebar_head_in`; Mail-Fenster torro und plex, hell und dunkel, breit/mittel/schmal angesehen
- **Modell:** Opus · **Braucht:** E11.1 · **Dateien:** `crates/torroinbox/src/{list,mail,search,app}.rs`,
  `crates/torro-gui/src/shell.rs`, `crates/torro-core/src/i18n/de.rs` (nur wenn neue Texte)
- **Leiste unter der gewählten Zeile** (Mail-Fenster, Kind `Mail`; Zeilen von Einträgen `list::entry_row` und von
  Ordnermails `mail::message_row`): statt `list::with_strip` neu `list::with_row_strip` mit `widgets::row_strip`, außerhalb
  des Innenrands der Zeile, von der linken bis zur rechten Kante, bündig am unteren Rand; zwischen Zeilen und Leiste
  `space::l()`. `list::sized_row` so umbauen, dass die Zeile (Knopf mit den Zeilen) und die Leiste untereinander
  liegen und der Auswahlbalken über beide reicht. Höhe mit Leiste: `row_message() + size::control_xl()`
  (`list::strip_height` für das Mail-Fenster). Zentrale und App Feed behalten `with_strip`; welche Leiste gilt, sagt
  ein neues Feld `edge_strip: bool` in `list::Rows` (und `mail::MailList`), das `App` für Kind `Mail` setzt. Die
  Höhen für das Scrollen zum Cursor (`list::row_span`, `height_of`) rechnen mit derselben Höhe.
- **Höhe seit #227:** Die Leiste ist `size::control_sm()` 28 hoch (mit der Linie 29) und steht anstelle der
  Vorschauzeile; die gewählte Zeile bleibt `row_message()` hoch, Absender und Betreff stehen wie in der ungewählten
  Zeile (`list::lines_top`), der Kontoname entfällt mit der Vorschau. Nur ein Design-Paket mit niedriger Zeile lässt
  sie um das Fehlende wachsen (`list::edge_strip_extra`). Die Angaben zu `control_xl()` und zu „Höhe mit Leiste“ oben
  sind der Stand von E11.2.
- **Reihenfolge** in der Leiste: Archive, Delete, Later, Task (Icon rot, `RowAction::accent`), Mute…, danach, was die
  Zeile heute sonst hat (AI task, Done/Reopen, Antworten einer Einladung vor Later). Einträge: `list::entry_actions`
  neu ordnen; Ordnermails: `mail::message_buttons` (das „…“ bleibt der letzte Knopf). Nichts fällt weg.
- **Wörter:** `widgets::row_strip_words` mit der Breite der Liste; bei 390 nur Icons, bei 520 Icons mit Wort (D).
- **Abstände:** Zeilen mit 16 oben, unten und links (`row_message` aus E11.1, `layout::row_padding` passt); die Hinweise
  Büroklammer, Fahne, Einladung zwischen Name und Zeit gibt es in Ordnerzeilen schon – nachgesehen; Einträge kennen
  Anhang und Fahne nicht (`Item` hat die Felder nicht) und zeigen weiter nur die Einladung.
- **Suchfeld** (`search::bar`): im Mail-Fenster das Feld über die volle Breite der Liste, `size::header()` hoch,
  `style::text_input_flat`, darunter `LINE`, ohne Rand links und rechts; Vorschläge, Bereich und Statuszeile
  darunter mit dem bisherigen Innenrand.
- **Lupe in der Seitenleiste entfällt** im Mail-Fenster: neue Funktion `shell::sidebar_head_in(mode, current, anchor, on,
  on_toggle, search: bool)`, `shell::sidebar_head` ruft sie mit `true` auf (Signatur bleibt); `App` übergibt
  `false` für Kind `Mail` (Ausklapp- und Leistenform). `/` und ⌘F/Strg+F fokussieren das Feld weiter.
- **Oberfläche:** Mail-Fenster, Liste. **API/MCP:** keine; dieselben Messages (`Message::Act`, `Message::Mail`).
- **Abnahme:** Tests: Reihenfolge der Leiste für Eintrag, Ordnermail und Einladung; nichts fehlt gegenüber vorher;
  Höhe der gewählten Zeile; Wörter bei 390 aus, bei 520 an; Zentrale zeigt weiter `with_strip`; Seitenleiste des
  Mail-Fensters ohne „Search“, die der Zentrale mit. Sichtprüfung (`torro-design-review`) Mail-Fenster in allen
  Größen, hell und dunkel, mit `--design plex`. Übersetzungen im Bericht.

### E11.3 Leser: Befehlsleiste, Kopf linksbündig, Pillen, Fuß mit Ansicht
- [x] erledigt (2026-10-16): `mail::{commands, command_cells, head, pills, pill_row, reply_menu, more_menu, attachment_line, foot}`, `Menu::Reply`/`Menu::More` mit `AskReplyMenu`/`AskMore`, Anker `reply`/`more`, Demo-Zustände `mail-reply`/`mail-more`; Torro-Leiste, `reader_head`, `bar_fit`, `attachment_card` entfernt; Leser bei 1320 und 1600, hell und dunkel, torro und plex angesehen
- **Modell:** Opus · **Braucht:** E11.1 · **Dateien:** `crates/torroinbox/src/{mail,detail,app}.rs`,
  `crates/torro-core/src/i18n/de.rs`, `docs/eingang.md` (Abschnitt zum Leser, falls er E9.4 beschreibt)
- **Aufbau von `mail::reader`** von oben nach unten: (1) Befehlsleiste fest, (2) Scrollbereich mit Kopf, Linie über
  die ganze Breite, Mail; (3) Anhänge fest; (4) Fuß fest. Die schwebende Torro-Leiste entfällt im Leser
  (`torro_groups`, `torro_fit`, `pack`, `torro_bar`, `page_end` und ihre Tests ersetzen oder entfernen;
  `widgets::floating_bar` bleibt in `torro-gui`).
- **Befehlsleiste** (`reader_head` ersetzt): `widgets::command_bar` mit Reply (`command_split`, `strong`, Menü →
  „Reply all“), Forward, Archive, Delete (aus im Papierkorb), Later (nur Eintrag, `AskSnooze`, Anker `snooze`),
  `command_more`. Wörter nach `widgets::command_words`. `bar_fit`/`BarFit` entfallen.
- **Menüs:** `Menu::Reply` und `Menu::More` in `app.rs`, geöffnet von neuen Messages `AskReplyMenu(String)` und
  `AskMore(String)` (Mail-Kennung, wie `AskMuteMail`); Anker `reply` und `more` in `detail::Anchors` (mit
  `Anchors::of`). „Reply all“ schickt `Message::Mail(MailAction::ReplyAll, …)`. „…“: Flag/Unflag, Mark read/unread,
  bei einem Eintrag Done/Reopen (`SetDone`) und Move to Info/For me (`AskLane`, die Frage öffnet am Anker `more`),
  im Leser Close (`CloseDetail`); jeder Eintrag mit seiner Taste als `kbd`. `Escape` schließt wie bei den anderen Menüs.
- **Kopf** (`reader_sender`): Betreff `typo::headline()`, darunter Avatar `size::pill()` und Name, darunter „to … ·
  Datum“ in `typo::meta()`/`text_2`; alles linksbündig an der Kante der Knöpfe, kein Datum rechts. Darunter die
  Pillen (`widgets::pill_button`): Task, AI task, Event, AI event (nur Eintrag; ohne KI aus mit Tooltip wie heute),
  Mute… (`AskHide` bzw. `AskMuteMail`); ohne Überschrift, umbrechend mit `space::s()`. Einladung und Aufgabe
  (`invite_card`, `task_card`) bleiben unter den Pillen. Grund der Spur (issue #128) bleibt unter dem Absender.
- **Mail:** über die volle Breite des Lesers, kein Rahmen; Text mit dem Innenrand der Seite, HTML von Rand zu Rand
  wie heute.
- **Anhänge:** eine Zeile `widgets::file_chip` (umbrechend), bündig über dem Fuß, oben `LINE`, Innenrand
  `space::s()` × Seitenrand; Klick wie heute `OpenAttachment`. `attachment_card` entfällt im Leser.
- **Fuß:** oben `LINE`, Fläche `bg`, Innenrand `space::s()` × Seitenrand, linksbündig: `view_switch` HTML/Text
  (`Icon::Code`, `Icon::AlignLeft`), `view_switch` Light/Dark (`Icon::Sun`, `Icon::Moon`, nur bei HTML), Copy text als
  `style::button::icon` (ohne Wort, Tooltip) – jeweils nur, wo `Reading::view_tools` es hat.
- **Oberfläche:** Leser des breiten Mail-Fensters. **API/MCP:** keine; dieselben Messages wie in E9.4.
- **Abnahme:** Tests: die Befehlsleiste hält genau Reply, Forward, Archive, Delete, Later (Eintrag), „…“; das Menü an
  Reply genau „Reply all“; „…“ genau Flag, gelesen, Done/Reopen und Move to (Eintrag), Close; die Pillen genau Task,
  AI task, Event, AI event, Mute… beim Eintrag und Task, Mute… bei der Ordnermail; jede Message wie vorher; kein
  roter Knopf im Leser (`style::button::primary` kommt nicht vor); Fuß ohne Schalter, solange die Mail lädt.
  Sichtprüfung (`torro-design-review`) im Leser bei 1320 und 1600 Fensterbreite, hell und dunkel, `--design plex`, mit
  HTML-Mail, Textmail, Mail mit zwei Anhängen und langer Mail. Übersetzungen im Bericht.

### E11.4 Paneel des Mail-Fensters wie der Leser
- [x] erledigt (2026-10-16): `mail::Bars` (`Split`/`Command`) an `mail::body` und `mail::panel`, `mail::entry_panel`, `mail::panel_foot`, `command_body`/`split_body`/`panel_letter`/`head_with`/`attachment_chips`, `detail::{path, heading}` geteilt, `App::{panel_bars, panel_width, mail_menu_sections}` („…“ im Paneel ohne Close); Mail-Fenster in Schmal und Mittel mit Eintrag, Ordner- und HTML-Mail, „…“- und Reply-Menü, hell und dunkel, torro und plex angesehen, Zentrale unverändert; Nachtrag: Befehlsleiste fest unter dem Kopf (`Panel::bar`, `mail::panel_bar`), Eigenschaften eines Eintrags unter dem Absender
- **Modell:** Opus · **Braucht:** E11.3 · **Dateien:** `crates/torroinbox/src/{mail,detail,app}.rs`,
  `crates/torro-gui/src/panel.rs` (Nachtrag: `Panel::bar`, feste Leiste unter dem Kopf),
  `crates/torro-core/src/i18n/de.rs` (nur wenn neue Texte)
- **Mittel und Schmal** im Mail-Fenster (`App::view` ohne `wide_reader`: `mail::panel` für Ordnermails,
  `detail::panel` mit `mail::body` für Mail-Einträge): neuer Parameter `mail::Bars` (`Split` = E9.4 für Zentrale und App
  Feed, `Command` = E11) an `mail::body` und `mail::panel`; der Kopf des Paneels (Pfad, Titel, ✕) bleibt. Mit
  `Command`: oben im Inhalt die Befehlsleiste aus E11.3 (ohne Close, das ✕ steht im Kopf), Absender und Pillen wie im
  Leser, Linie, Mail, Anhänge als Chips; der Fuß des Paneels (`Panel::footer`) ist der Fuß aus E11.3 statt
  `detail::Footer`/`folder_footer`. Die Teile aus E11.3 als gemeinsame Funktionen in `mail.rs` nutzen, nicht doppelt bauen.
- **Oberfläche:** Mail-Fenster in Mittel und Schmal. **API/MCP:** keine.
- **Abnahme:** Tests: Paneel des Mail-Fensters mit Befehlsleiste, Pillen und Fuß aus E11.3 für Eintrag und Ordnermail;
  Paneel der Zentrale (`Kind::Hub`) unverändert wie in E9.4. Sichtprüfung (`torro-design-review`) Mail-Fenster in
  Schmal (480×900) und Mittel (800×600), hell und dunkel, und die Zentrale zum Vergleich. Übersetzungen im Bericht.

## E13 Dichte kommt vom Design (Issue #226, Plan 2026-10-19)

Wie dicht die Einträge stehen, ist kein Fensterzustand mehr, sondern ein Wert des Designs
(`torro_core::design::Design`). Der Umschalter „Compact | Comfortable“ aus E8.2, der Startschalter `--density` und der
Schlüssel `density` in `ui.json` fallen weg. Die Bausteine bleiben: `widgets::card`, `card_head`, `source_tile`,
`chip_row`, `typo::read()`, die Maße der Karten und alle Comfortable-Pfade in Mail, App Feed und Zentrale. Regeln:
[CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`, Oberfläche: Skill `torro-design`.

**Keine Funktion des Dienstes** (wie E8.2): Die Dichte ist Darstellung, kein Request und kein MCP-Tool. Als Teil des
Designs ist sie über den bestehenden Weg erreichbar – Oberfläche: Appearance › Design in der Steuerzentrale, API:
`settings.set_setting design` und `settings.list_designs`, MCP: dieselben Tools.

Entschieden am 2026-10-19 (Planung #226):

1. **Schlüssel `layout.density`**, kein eigener Abschnitt: `[layout]` trägt schon Ränder und Lücken, und `Design::layout`
   ist das Feld dazu. Verworfen: `[inbox]` oder `[density]` – ein Abschnitt für einen einzigen Wert, der zudem nach einer
   App statt nach einer Rolle hieße.
2. **Beide Werte bleiben** in `Density` (`Compact`, `Comfortable`), obwohl kein mitgeliefertes Paket `comfortable` setzt:
   sonst wäre der ganze Kartencode unerreichbar und müsste mit weg. Das Entfernen der Karten ist ein eigenes Thema.
3. **Kein mitgeliefertes Paket zeigt Karten** (in der Planung entschieden): torro, plex, ribbon, quattro … bleiben
   kompakt, keines bekommt `layout.density`. Comfortable zeigt nur ein selbst gebautes Paket; Tests und Sichtprüfung
   nehmen dafür ein Temp-Paket (`TORRO_DATA_HOME=<tmp>`, Paket unter `<tmp>/torroworkplace/designs/<id>/design.toml`,
   Start mit `--demo --design <id>`). Verworfen: Plex auf `comfortable` – Plex sähe dann anders aus als sein
   `preview.png`, und jede Sichtprüfung mit Plex zeigte etwas anderes als bisher.
4. **Keine neue Regel in `design/rules.rs`:** Ein Wort, das nicht `compact` oder `comfortable` ist, lehnt schon der
   dritte Lesedurchgang ab (`DesignError::Syntax` mit Zeile), wie bei `type.*.font` und `weight`. Einen Bereich wie bei
   Zahlen gibt es hier nicht; `rules.rs` bekommt nur einen Satz im Modul-Kommentar.
5. **Das Design entscheidet in jeder Größenklasse:** `ui_state::Density::default_for` (Schmal immer kompakt) entfällt.
   Bisher galt eine Wahl des Menschen in jeder Größe und die Größe entschied nur, solange niemand gewählt hatte; die
   Wahl des Designs gilt genauso. Mini und Leiste bleiben Anzeigen ohne Dichte.
6. **Das Fenster hält die Dichte als Feld** (`App::density`), gesetzt beim Start aus dem aktiven Design und neu gelesen,
   wenn `Message::Design` ein anderes Design bringt – der einzige Weg, auf dem sich das Design in einem App-Prozess
   ändert (`design::start_for` läuft vor dem ersten Fenster). Verworfen: in jedem Frame `layout::density()` lesen – dann
   hinge jeder Test an dem einen Design, das alle Tests eines Binärprogramms teilen, und ein Test, der auf Karten
   umschaltet, würde die Tests neben sich verfälschen.

```
E13.1 ── E13.2 ─┬─ E13.3
                └─ E13.4
```

Nicht parallel zu E9.2, E9.3b und E11.x: E13.2 ändert `crates/torroinbox/src/app.rs` an vielen Stellen.

### E13.1 Dichte als Wert des Designs und Schlüssel des Paket-Formats
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-core/src/design.rs`,
  `crates/torro-core/src/design/{pack,rules}.rs`, `crates/torro-ipc/src/api.rs` (nur Beschreibungen),
  `docs/api-reference.md` (erzeugt), `docs/design-pakete.md`
- **`Density`** in `torro_core::design`: `Compact`, `Comfortable`, mit `Debug, Clone, Copy, PartialEq, Eq,
  serde::Deserialize` und `#[serde(rename_all = "lowercase")]`, wie `FontRole` und `Weight` daneben; kein `Display`,
  weil niemand die Dichte ausgibt. Doc-Kommentar: was die beiden Werte im Eingang bedeuten (eine Zeile je
  Eintrag / Karten mit Vorschau und Knöpfen) und dass die Wahl zum Design gehört, nicht zum Fenster.
- **Feld:** `Layout::density: Density` (`Layout` bleibt `Copy`), `Design::standard()` setzt `Density::Compact`.
- **Paket-Format** (`design/pack.rs`): `LayoutFile` bekommt `density: Option<Density>`, `LAYOUT_KEYS` wächst auf 8 mit
  `"density"` (damit `known()` den Schlüssel kennt und ein Tippfehler `UnknownKey` bleibt), `merge_layout` setzt
  `l.density = file.density.unwrap_or(l.density)`. Fehlt der Schlüssel, gilt `compact` aus dem Standard.
- **`design/rules.rs`:** keine Prüfung (Entscheidung 4); im Modul-Kommentar ein Satz, warum die Dichte keine hat.
- **`docs/design-pakete.md`:** im `[layout]`-Beispiel `density = "compact"` mit Kommentar („Zeilen oder Karten im
  Eingang, E8.2/#226“); in der Regel-Tabelle die Zeile „Layout-Abstände“ um `density` nur `compact` oder `comfortable`
  ergänzen; der Absatz „Layout-Abstände sind zuerst allgemein“ nennt die Dichte; im Ausblick auf Stufe 2 die Sätze
  streichen, die die Dichte dort noch erwarten (Kandidat „Dichte“, „Listen als Zeilen oder Karten“).
- **Befehlskatalog** (aus der API-Prüfung des Plans, `crates/torro-ipc/src/api.rs`): kein neuer Eintrag, aber zwei
  Beschreibungen nennen die Nebenwirkung – `LIST_DESIGNS` zählt auf, was ein Paket setzt („colours, fonts, spacing,
  logo“) und bekommt die Dichte des Eingangs dazu (Zeilen oder Karten); bei `SET_SETTING` der Halbsatz zu `design`
  ebenso. Danach `TORRO_BLESS=1 cargo test -p torro-ipc` und das erzeugte `docs/api-reference.md` mitcommitten.
- **Oberfläche:** keine. **API/MCP:** kein neuer Request und kein neues Tool (die Dichte kommt mit dem Design,
  `settings.set_setting design`, `settings.list_designs`).
- **Abnahme:** Tests in `design.rs` und `design/pack.rs`: `Design::standard().layout.density == Density::Compact`;
  ein Temp-Paket mit `[layout]\ndensity = "comfortable"` lädt und liefert `Comfortable`; ein Paket ohne den Schlüssel
  liefert `Compact`; `density = "roomy"` wird mit `DesignError::Syntax` abgelehnt (Zeile gesetzt); `layout.dense` bleibt
  `DesignError::UnknownKey`. Die vorhandenen Tests der mitgelieferten Pakete (`torroworkplace/src/designs.rs`) bleiben
  grün; einer davon prüft zusätzlich, dass das Paket kompakt bleibt. `cargo fmt`, `clippy -D warnings`, `cargo test`,
  `TORRO_LANG=de cargo test`.

### E13.2 Der Eingang zeichnet in der Dichte des Designs
- [x] erledigt
- **Modell:** Opus · **Braucht:** E13.1 · **Dateien:** `crates/torro-gui/src/layout.rs`,
  `crates/torroinbox/src/{app,hub,notifications,main}.rs`
- **Token:** `torro_gui::layout::density()` → `design::with(|t| t.design.layout.density)`, Doc-Kommentar wie die anderen
  Rollen dieses Moduls (Standard: kompakt). Dichte des Designs, nicht des Fensters.
- **`Density` kommt aus dem Kern:** In `app.rs`, `hub.rs` und `notifications.rs` `use torro_core::design::Density`
  statt `torro_gui::ui_state::Density`. Die Signaturen von `hub::layout`, `notifications::{board, height, span}`,
  `notifications::BoardData::density` und `hub::Layout::cards` bleiben unverändert – sie nehmen die Dichte weiter als
  Parameter, nur ihre Quelle ändert sich.
- **`App`:** Feld `density: Density` (kein `Option` mehr), in `App::new` aus `layout::density()`, im Arm
  `Message::Design` nach `self.design.update(message)` neu gelesen (vor dem `return` des Tasks). `App::density()` gibt
  das Feld zurück. Weg: `Launch::density`, der Schalter `--density` in `Launch::parse`, `Message::SetDensity`,
  `Message::StoredDensity`, `App::set_density`, der `load_density_task`-Zweig in `App::new` und
  `Density::default_for`.
- **`Launch::density_key()` → `Launch::window_key()`:** gleicher Wert, neuer Name; Aufrufer sind `order` und `split`
  (`load_order_task`, `save_order_task`, `load_split_task`, `save_split_task`) und die Tests. Doc-Kommentar ohne
  Dichte: der Schlüssel benennt das Fenster und seinen Ausschnitt.
- **Kopfleiste** (`App::header`): die Zeile mit `widgets::density_switch` entfällt. Die Regel für `words`
  (Breit, oder Mittel ohne „Mail | Notifications“ und ohne „By app | By time“) bleibt und gehört jetzt dem
  Knopf „Newest first | Oldest first“ (`order_button`); Kommentar nachziehen.
- **Doc-Kommentare** oben in `app.rs` und `main.rs`: kein Umschalter, kein `--density`, kein Merken; die Dichte kommt
  vom Design.
- **Oberfläche:** ja – Kopfleiste aller drei Fensterarten, Zeilen statt Karten im Design „Torro“.
  **API/MCP:** keine.
- **Abnahme:** Tests in `app.rs`. Weg: `density_is_read_from_the_command_line`,
  `density_follows_the_command_line_then_ui_json_then_the_layout`, `every_window_kind_draws_the_density_switch`. Neu
  bzw. umgebaut:
  - Je Fensterart (`hub`, `mail`, `notifications`) zeigt die Kopfleiste keinen Dichte-Umschalter mehr, in allen
    Größenklassen; der Knopf der Reihenfolge behält sein Wort in Breit und Mittel.
  - `Launch::parse(&["--density", "comfortable", "--view", "mail"])` ist `Ok` und ignoriert den Schalter (unbekannte
    Wörter lässt `parse` liegen), `window_key()` bleibt `mail`.
  - `App::density()` ist `Compact`, solange das Design „Torro“ aktiv ist; nach einem Wechsel auf ein Temp-Paket mit
    `layout.density = "comfortable"` (wie in `the_stored_design_is_drawn_and_a_broken_one_gives_the_standard`:
    `Follow::new(None).in_dir(…)` und `Message::Design(design::Message::Stored(…))`) ist sie `Comfortable`. Dieser
    Test – der einzige, der das Design des Prozesses umschaltet – stellt am Ende `design::set(Design::standard())`
    wieder her.
  - Damit kein anderer Test am Design des Prozesses hängt, setzen die Helfer `demo`/`loaded`/`overview_at` die Dichte
    nach dem Bau ausdrücklich auf `Density::Compact`; `mail_window` nimmt statt des Wortes ein `Density` und setzt das
    Feld, `notifications_window` und die Tests, die bisher `--density comfortable` übergaben, setzen es ebenso.
  - Tests, die bisher ohne `--density` liefen, bekamen in Breit Comfortable und bekommen jetzt Compact: Wo eine Abnahme
    Karten prüft, im Test die Dichte auf `Comfortable` setzen – keine Abnahme abschwächen. In `hub.rs` und
    `notifications.rs` bleiben die Tests, die die Dichte als Parameter übergeben, unverändert.
  - `cargo fmt`, `clippy -D warnings`, `cargo test`, `TORRO_LANG=de cargo test`, `scripts/design-check.sh --strict`.
  - Sichtprüfung (`torro-design-review`): die drei Fensterarten (`--view hub|mail|notifications --demo`) in allen
    Größen, hell und dunkel – Zeilen, keine Kopfleiste mit „Compact | Comfortable“; dazu dieselben Fenster comfortable
    über ein Temp-Paket (`TORRO_DATA_HOME=<tmp>`, `--demo --design <id>`), damit die Karten nachweislich noch zeichnen.

### E13.3 Umschalter, `Density` und `density` in `ui.json` entfernen
- [x] erledigt
- **Modell:** Haiku · **Braucht:** E13.2 · **Dateien:** `crates/torro-gui/src/{ui_state,widgets}.rs`,
  `crates/torro-gui/examples/gallery.rs`, `crates/torro-core/src/i18n/de.rs`
- **`ui_state.rs`:** weg sind `Density`, die Konstante `DENSITY`, `density_from`, `save_density_to`,
  `load_density_task`, `save_density_task` und der dann ungenutzte `use crate::layout::SizeClass`. Der Modul-Kommentar
  nennt die Dichte nicht mehr; die Doc-Verweise „Windows are keyed as for `density_from`“ in `order_from` und
  `split_from` zeigen auf `order_from`. `order` und `split` bleiben unverändert – ein `ui.json` mit altem `density`
  lädt weiter, der Schlüssel wird nur nicht mehr gelesen oder geschrieben (keine Migration).
- **Tests in `ui_state.rs`:** `densities_are_kept_per_window_beside_the_rest` und `density_words_and_defaults` weg;
  `orders_are_kept_per_window_beside_the_rest` beweist „beside the rest“ mit `save_to`/`save_unfolded_to` statt mit
  der Dichte. Neu oder ergänzt: Ein `ui.json`, das `density` enthält, verliert weder `order` noch `split`, und eine
  Schreiboperation fügt `density` nicht hinzu.
- **`widgets.rs`:** `density_switch` und `use crate::ui_state::Density` weg, der Verweis im Modul-Kommentar ebenso;
  der Test `cards_tiles_and_the_density_switch_build` prüft nur noch Karten und Kacheln (Name nachziehen).
- **`examples/gallery.rs`:** Import, Feld `density`, `Msg::Density` und der Abschnitt „Compact | Comfortable“ der
  Karten-Seite (`--cards`) weg.
- **`i18n/de.rs`:** die Einträge `("Compact", "Kompakt")` und `("Comfortable", "Komfortabel")` samt Gruppenkommentar
  weg (kein `tr()` verweist mehr darauf).
- **Oberfläche:** Galerie. **API/MCP:** keine.
- **Abnahme:** `grep -rniI "density" crates/torro-gui crates/torro-core/src/i18n` findet nur noch die Dichte des
  Designs (`layout::density`); `cargo fmt`, `clippy --all-targets -D warnings` (kein ungenutzter Import),
  `cargo test`, `TORRO_LANG=de cargo test`; `cargo run --example gallery -- --cards` baut und zeigt die Karten-Seite
  ohne den Umschalter.

### E13.4 Dokumente und Skill: die Dichte gehört zum Design
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** E13.2 · **Dateien:** `docs/eingang.md`, `docs/settings.md`,
  `docs/tasks/eingang.md`, `.claude/skills/torro-design/SKILL.md`
- **`docs/eingang.md`:** Abschnitt „Zwei Dichten“ wird „Dichte“: ein Wert des Designs für die ganze Suite, „Torro“
  zeigt eine Zeile je Eintrag, Karten zeigt ein Paket mit `layout.density = "comfortable"`; kein Umschalter, kein
  `--density`, nichts Gemerktes. Die Bausteine der Karten bleiben beschrieben. In den Tabellen weiter unten („E8 Drei
  Fenster“, Punkt 13 „Dichte“) die Wahl je Fenster streichen.
- **`docs/settings.md`:** in der Liste des Fensterzustands die Dichte streichen (Seitenleiste, Breite, Reihenfolge
  bleiben); in der Tabelle der Startschalter `--density` entfernen; bei den Schlüsseln von `ui.json` `density`
  entfernen und den Satz dazu nachziehen.
- **`docs/tasks/eingang.md`:** bei E8.2 ein Nachtrag in einer Zeile, dass Umschalter, `--density` und `density` in
  `ui.json` mit E13 (Issue #226) weggefallen sind und die Dichte zum Design gehört; in der Liste der Startschalter bei
  der alten Aufgabe `--density` streichen. Abgehakte Abnahmen nicht umschreiben.
- **`.claude/skills/torro-design/SKILL.md`:** der Abschnitt „Zwei Dichten im Eingang“ beschreibt die Dichte als Wert
  des Designs (`torro_core::design::Density`, `torro_gui::layout::density()`, `layout.density` im Paket), Standard
  kompakt, Karten nur mit einem Paket; Umschalter, `--density` und `Density::default_for` streichen. Der Hinweis, alle
  Bausteine beider Dichten anzuschauen, bleibt.
- **Oberfläche:** keine. **API/MCP:** keine.
- **Abnahme:** `grep -rniI -- "--density|ui_state::Density|density_switch" docs .claude` findet nur noch die Stellen,
  die das Wegfallen beschreiben (Nachtrag bei E8.2 und die Aufgaben von E13); die vier Dokumente beschreiben die Dichte
  als Wert des Designs; `scripts/ci-local.sh` grün.

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


## E10 Tastenkürzel im Mail-Fenster (vorgemerkt 2026-10-15)

Entschieden im Gespräch am 2026-10-15. Die Kürzel stellt man in den **Einstellungen des Mail-Fensters** ein: Man wählt
eine Vorlage und kann jeden Befehl darüber hinaus frei belegen. **E10.2 und E10.3 warten auf die Einstellungen im
Mail-Fenster:** [S.16](steuerzentrale.md#s16-gerüst-bibliothek-torro-settings-ui-und-host-für-den-inhaltsbereich) (Gerüst
`torro-settings-ui`) und [S.20](steuerzentrale.md#s20-eingang-und-mail-einstellungen-im-inhaltsbereich) (Eingang und Mail).
E10.1 und E10.1b brauchen sie nicht und können vorher laufen.

Geändert im Gespräch am 2026-10-16 (#200): Jedes System bekommt die Kürzel seines gewohnten Mailprogramms als Vorgabe,
**macOS Apple Mail, Linux Thunderbird, Windows Outlook** (E10.1b). „Torro“ ist nirgends mehr Vorgabe, bleibt aber
wählbar. Jede Vorlage geht auf jedem System, etwa die Kürzel von Apple Mail unter Linux für alle, die vom Mac kommen.

**Grundsätze**

- Vorgabe je System: macOS Apple Mail, Linux Thunderbird, Windows Outlook. In den Einstellungen wählt man eine andere
  Vorlage (auch die eines anderen Systems) und belegt einzelne Befehle selbst (E10.2, E10.3).
- Unter Windows und Linux gilt Strg, auf dem Mac ⌘ und gleichwertig Strg: beide gehen. Eine Vorlage eines anderen
  Systems wird übersetzt: ⌘ wird unter Linux und Windows zu Strg (Super gehört unter Hyprland dem Fenstermanager).
- Entf und ⌫ löschen auf jedem System (gibt es seit E8.18). ⇧+Entf, auf dem Mac auch ⇧+⌫, löscht endgültig. Davor
  kommt eine Rückfrage, weil es kein Undo gibt. Die Rückfrage gehört zum Befehl, nicht zur Taste: Sie kommt bei jeder
  Belegung.
- Die Einzeltasten (`r`, `a`, `f`, `e`, `#`, `/`, `j`/`k` …) bleiben in jeder Vorlage zusätzlich.
- Kürzel wirken nur, wenn kein Textfeld den Fokus hat; dort gelten dessen Tasten (Kopieren, Einfügen, Alles auswählen).
  Ausnahme: Senden im Schreibfenster.

**Vorlage „Torro“ (wählbar; Vorgabe nur, bis E10.1b steht)** – gängige Kürzel, sonst die von Apple Mail, Verschieben
wie in Outlook, auf jedem System gleich:

| Befehl | Windows, Linux | macOS (⌘ oder Strg) |
|---|---|---|
| Neue Mail | Strg+N | ⌘N |
| Antworten | Strg+R | ⌘R |
| Allen antworten | Strg+⇧+R | ⇧⌘R |
| Weiterleiten | Strg+⇧+F | ⇧⌘F |
| Senden (Schreibfenster) | Strg+Enter | ⌘Enter |
| Löschen | Entf, ⌫ | ⌫, Entf |
| Endgültig löschen | ⇧+Entf | ⇧⌫, ⇧+Entf |
| Archivieren | Strg+⇧+A | ⌃⌘A |
| Gelesen/ungelesen | Strg+⇧+U | ⇧⌘U |
| Markieren | Strg+⇧+L | ⇧⌘L |
| Spam | Strg+⇧+J | ⇧⌘J |
| In Ordner verschieben | Strg+⇧+V | ⇧⌘V |
| Suchen | Strg+F | ⌘F |
| Mails abrufen | Strg+⇧+N | ⇧⌘N |
| Rückgängig | Strg+Z | ⌘Z |

Strg+A bleibt „Alles auswählen“, darum Archivieren mit Strg+⇧+A. Unter Linux fängt IBus Strg+⇧+U ab, solange eine
Eingabemethode aktiv ist (Unicode-Eingabe); bei der Sichtprüfung unter Hyprland ansehen. Rückgängig gilt in der ganzen
Suite (`widgets::is_undo_key`) und ist nicht frei belegbar.

**Weitere Vorlagen:** Apple Mail, Outlook, Thunderbird (E10.1b), Gmail (E10.2), jeweils wie beim Vorbild. ⌘ und Strg
werden je System übersetzt. Die Belegungen vor der Umsetzung gegen die aktuelle Hilfe des Herstellers prüfen; die Übersicht im Gespräch
war nicht nachgeschlagen. Was das Vorbild nicht kennt (Später, Aufgabe, Spur …), bleibt bei den Einzeltasten.

Stand E10.1b (gegen die Hilfe von Apple, Mozilla und Microsoft geprüft am 2026-10-16; Quellen und Gründe im Doc-Kommentar
der Tabellen in `torro_core::shortcuts`). Erste Taste je Befehl, weitere in Klammern; „wie Torro“ heißt: Das Vorbild kennt
keine Taste dafür oder seine kollidiert, Löschen und endgültig Löschen sind überall wie in „Torro“:

| Befehl | Apple Mail (macOS) | Thunderbird (Linux, Windows) | Outlook (Windows, Linux) |
|---|---|---|---|
| Neue Mail | ⌘N | Strg+N (Strg+M) | Strg+N |
| Antworten | ⌘R | Strg+R | Strg+R |
| Allen antworten | ⇧⌘R | Strg+⇧+R | Strg+⇧+R |
| Weiterleiten | ⇧⌘F | Strg+L | Strg+F |
| Senden | ⇧⌘D (⌘Enter) | Strg+Enter | Strg+Enter |
| Archivieren | ⌃⌘A | Strg+⇧+A, wie Torro (Vorbild: A) | Strg+⇧+A, wie Torro |
| Gelesen/ungelesen | ⇧⌘U | Strg+⇧+U, Linux Strg+U (Vorbild: M) | Strg+U (Strg+Q) |
| Markieren | ⇧⌘L, wie Torro | Strg+⇧+L, wie Torro (Vorbild: S) | Einfg |
| Spam | ⇧⌘J | Strg+⇧+J, wie Torro (Vorbild: J) | Strg+⇧+J, wie Torro |
| In Ordner verschieben | ⇧⌘V, wie Torro | Strg+⇧+V, wie Torro | Strg+⇧+V |
| Suchen | ⌥⌘F (⌘F; ⌥⌘F aus dem Menü, nicht aus der Hilfe) | Strg+K (Strg+⇧+K, Strg+F) | Strg+E (F3) |
| Mails abrufen | ⇧⌘N | F5 (⇧+F5) | F9 (Strg+M) |

Auf den anderen Systemen: Apple Mail unter Windows und Linux mit Strg statt ⌘, Archivieren Strg+⇧+A (⌃⌘A wäre Strg+A),
unter Linux Gelesen/ungelesen Strg+U (IBus), Suchen nur Strg+F (Strg+Alt ist unter Windows AltGr). Thunderbird auf dem Mac mit ⌘ nach der Mac-Spalte der Hilfe,
Archivieren ⌃⌘A wie Torro. Outlook auf dem Mac ist Outlook für Windows mit ⌘, aber Gelesen ⌃Q (⌘Q beendet),
Markieren ⇧⌘L wie Torro (kein Einfg), ohne F3 und F9. **Abrufen ist auf dem Mac in jeder Vorlage ⇧⌘N wie in Torro**
(entschieden im Review von PR #214): F-Tasten haben auf dem Mac keine Entsprechung, und was das Vorbild dafür nennt –
Thunderbirds ⌘Y, Outlooks ⌃M – ist kaum zu erraten; dieselbe Regel gibt Outlook auf dem Mac Archivieren ⌃⌘A statt ⇧⌘A.
Gesperrt für alle Vorlagen und für eigene Tasten (E10.2): `torro_core::shortcuts::reserved`.

**Frei belegen:** Eigene Tasten liegen über der gewählten Vorlage. Ein Wechsel der Vorlage behält sie, „Reset“ löscht
eine oder alle. Ist eine Taste schon belegt, auch als Einzeltaste (`r`, `a`, `j`/`k` …), kommt ein Hinweis mit
„Replace“. Nicht belegbar: Esc, `[`, Zoom, Rückgängig, Kopieren, Ausschneiden, Einfügen, Alles auswählen.

### E10.1 Kürzel-Tabelle und Vorlage „Torro“
- [x] erledigt (2026-10-15: `torro_core::shortcuts` hält Befehle (snake_case-Namen für E10.2), Schreibweise
  (`Mod`/`Ctrl`/`Cmd`/`Alt`/`Shift`, Parsen, Ausgeben, Hinweise je System) und die Vorlage „Torro“ nach der Tabelle.
  `keys::shortcut` hört Strg/⌘-Kürzel auch, wenn ein Widget sie fing; ob das Suchfeld den Fokus hat, fragt die App bei
  Iced nach (`operation::is_focused`), weil `ctrl_guard` und Textfelder den Status unbrauchbar machen – nur Suchen und
  Senden wirken dort. Entf/⌫ und ⇧+Entf (Mac auch ⇧+⌫) laufen weiter als Einzeltasten über die Tabelle. Neu: Archivieren
  überall, Gelesen/ungelesen und Markieren nach Zustand, Spam und „Move to folder“ (Auswahlmenü mit Ziffern) mit Undo,
  endgültig löschen hinter der Rückfrage „Delete permanently?“ (Client: erst in den Papierkorb, dann `permanent: true`),
  neue Mail, abrufen (`accounts.sync`, dann neu laden). Auf dem Mac gelten ⌘ und Strg gleich, auch für Rückgängig und
  Senden. Die Hinweise in den Aktionsleisten bleiben die Einzeltasten; „Send“ nimmt seinen Hinweis aus der Tabelle.
  `--open move-mail|delete-mail` für die Sichtprüfung. Auf main gebracht 2026-10-16 (#200), ID von E9 auf E10.)
- **Modell:** Opus · **Braucht:** E8.18 · **Dateien:** neu `crates/torro-core/src/shortcuts.rs` (Befehle, Tasten,
  Vorlage „Torro“; ohne Iced, damit der Kern in E10.2 dieselbe Tabelle prüft), `crates/torro-core/src/lib.rs`,
  `crates/torroinbox/src/{keys,app,mail,compose}.rs`, `crates/torro-gui/src/widgets.rs` (Rückgängig auf dem Mac auch
  mit Strg+Z), `crates/torro-ipc/src/inbox.rs` (Client-Aufrufe für `Sync` und `DeleteMail` mit `permanent`)
- `keys` liest die Kürzel aus der Tabelle; die Einzeltasten bleiben. Neue Befehle: Archivieren in jedem Fenster,
  Gelesen/ungelesen, Markieren, Spam, Verschieben (öffnet die Ordnerwahl als Auswahlmenü, wie „Later“), endgültig
  löschen, neue Mail, abrufen.
  Tastenhinweise an Knöpfen und in Menüs kommen aus derselben Tabelle; in den Aktionsleisten bleiben vorerst die
  Einzeltasten stehen, nur „Send“ zeigt sein Kürzel (die Hinweise der Tabelle mit E10.2, wenn Tasten frei belegbar sind).
- **Oberfläche:** Mail-Fenster (Liste, Leser), Schreibfenster. **API:** vorhandene Requests, kein neuer:
  `mail.move_messages` · Write (Archivieren mit Rolle `archive`, Spam mit Rolle `junk`, Verschieben),
  `mail.delete_messages` · Delete (ohne und mit `permanent: true`), `mail.set_flags` · Write, `mail.search_messages` ·
  Read, `mail.save_draft` und `mail.send_draft` · Write, `accounts.sync` · Write (gleicht alle Konten ab, nicht nur
  Mail). **MCP:** keine neuen Tools – Tasten sind Oberfläche, die Befehle dahinter haben ihre Tools.
- **Abnahme:** Tests je Kürzel für beide Systeme (macOS: ⌘ und Strg), Strg+A archiviert nicht, ⇧+Entf fragt erst,
  Textfeld behält seine Tasten, Einzeltasten wie bisher; beide Sprachen.

### E10.1b Vorlagen Apple Mail, Thunderbird und Outlook als Vorgabe je System
- [x] erledigt (2026-10-16, #209: Vorlagen nach der Tabelle unter „Weitere Vorlagen“, `Preset::standard` je System,
  `Preset::default()` folgt `System::current()`. `Key` kennt F1–F12 und Einfg; diese wirken ohne Strg. In Outlook
  schalten Strg+Q und Strg+U beide um. Suchen und Senden behalten Strg+F und Strg+Enter, wo die Vorlage sie nicht
  anders belegt, in Outlook leitet Strg+F weiter. Auf dem Mac nimmt jeder Befehl ohne eigene Mac-Taste die aus „Torro“,
  auch wenn das Vorbild eine nennt: Abrufen überall ⇧⌘N, Outlook archiviert ⌃⌘A (Review von PR #214). Gesperrte Tasten als `shortcuts::reserved` für E10.2. Apple Mails
  Hilfe nennt keine Suchtaste, ⌥⌘F kommt aus dem Menü. Thunderbird auf dem Mac nach der Mac-Spalte seiner Hilfe.)
- **Modell:** Opus · **Braucht:** E10.1 · **Dateien:** `crates/torro-core/src/shortcuts.rs`,
  `crates/torroinbox/src/{keys,app,compose}.rs`, `crates/torro-gui/src/widgets.rs` (Rückgängig unterscheidet `System`)
- Drei neue Vorlagen in `torro_core::shortcuts`, jede gegen die aktuelle Hilfe des Herstellers geprüft (Quelle und Datum
  im Doc-Kommentar der Vorlage). Die Vorgabe richtet sich nach dem System: macOS Apple Mail, Linux Thunderbird, Windows
  Outlook. Dafür unterscheidet `System` Windows und Linux, nicht mehr nur `Mac` und `Other`; das ändert eine
  festgelegte Schnittstelle und ist hiermit beschlossen. Jede Vorlage hat Tasten für alle Systeme, damit E10.2 sie
  überall anbieten kann; ⌘ wird unter Linux und Windows zu Strg.
- Wo die Übersetzung oder das Vorbild mit festen Tasten kollidiert (etwa ⌃⌘A, das unter Linux zu Strg+A, also „Alles
  auswählen“, würde, oder Strg+⇧+U und IBus), bekommt der Befehl eine freie Ersatztaste; die Wahl steht im Doc-Kommentar.
  Was das Vorbild nicht kennt (Spam, In Ordner verschieben, Später …), übernimmt die Vorlage von „Torro“, damit jeder
  Befehl eine Taste hat. Rückfrage vor dem endgültigen Löschen und die Einzeltasten bleiben.
- **Oberfläche:** Mail-Fenster und Schreibfenster, wie E10.1; ohne Einstellung, die Vorgabe ist fest bis E10.2.
  **API/MCP:** kein neuer Request (wie E10.1); die Auswahl kommt mit E10.2.
- **Abnahme:** Tests je Vorlage und System (Tabelle gegen die Hilfe des Herstellers), keine Doppelbelegung in einer
  Vorlage, auch nicht mit den Einzeltasten und den gesperrten Tasten; die Vorgabe folgt `System::current()`; beide
  Sprachen.

### E10.2 Vorlagen wählen und eigene Tasten im Kern
- [ ] offen – wartet auf S.20
- **Modell:** Opus · **Braucht:** E10.1b, S.20 · **Dateien:** `crates/torro-core/src/{shortcuts,settings}.rs`,
  `crates/torro-ipc/src/{lib,api}.rs` (`SettingKey`, Beschreibungen von `SET_SETTING` und `GET_SETTINGS`),
  `crates/torroworkplace/src/{core,approvals,mcp}.rs` (Freigabe-Text; in `mcp.rs` nur der Kommentar zu `narrow_schema`),
  `crates/torroinbox/src/{keys,app,compose}.rs`, `docs/api.md`, `docs/api-reference.md`
- Vorlage Gmail (gegen die Hilfe des Herstellers geprüft). Ohne gespeicherte Wahl gilt die Vorgabe des Systems aus
  E10.1b; wählbar sind alle Vorlagen auf jedem System. Der Kern prüft
  Schreibweise, gesperrte Tasten und Doppelbelegung, auch mit den Einzeltasten (`conflict` mit dem belegten Befehl).
  Offene Mail-Fenster folgen sofort, wie beim Design.
- **Schreibweise:** `Mod` steht für Strg unter Windows und Linux und für ⌘ oder Strg auf dem Mac (`"Mod+Shift+A"`);
  `Ctrl`, `Cmd`, `Alt`, `Shift` gelten wörtlich. Die gültigen `command`-Namen stehen im Doc-Kommentar von `SettingKey`
  und in der Katalogbeschreibung, damit Agenten sie kennen.
- **Freigabe:** Agenten brauchen für Admin immer eine Freigabe. Ihr Text ist lesbar statt rohem JSON, etwa „Bind Shift+J
  to Delete permanently“ bzw. „Switch mail shortcuts to Outlook“, und nennt bei Lösch-Befehlen ausdrücklich, dass gelöscht
  wird (`approvals.rs`). Eigener Zweig vor dem allgemeinen `Request::SetSetting { key, value }`, sonst griffe „Change the
  setting … to {JSON}“ mit dem Text des Clients. Der Text entsteht erst nach der Prüfung im Dienst: Befehls- und
  Vorlagennamen aus der Tabelle in `torro_core::shortcuts`, Tasten in der Schreibweise, die der Dienst daraus macht, nie
  der rohe `value`. Bei `"replace": true` nennt er den Befehl, dem die Taste weggenommen wird, aus dem Stand des Dienstes
  („Bind Shift+J to Delete permanently (taken from Spam)“); bei `{"reset": "all"}` sagt er, wie viele eigene Tasten
  verloren gehen (aus der API-Prüfung, #200).
- **Oberfläche:** E10.3. **API:** `Request::SetSetting` · `settings.set_setting` · Admin, neue Schlüssel
  `mail_shortcut_preset` (`"torro"`, `"apple_mail"`, `"outlook"`, `"thunderbird"`, `"gmail"`) und `mail_shortcut`
  (`{"command": "archive", "keys": "Mod+Shift+A"}`; `"keys": null` = zurück zur Vorlage; `"replace": true` nimmt die
  Taste dem anderen Befehl im selben Schritt weg; `{"reset": "all"}` löscht alle eigenen Tasten); klein und gezielt wie
  `list_order`. Doc-Kommentar und Katalogbeschreibung nennen beide Formen von
  `mail_shortcut` und die Fehlercodes (`conflict` mit dem belegten Befehl, `error` für gesperrte Tasten und falsche
  Schreibweise). `settings.get_settings` liefert `mailShortcuts` (Vorlage, eigene Tasten und die daraus wirksame
  Belegung, damit kein Client sie selbst zusammensetzt) · Read. **MCP:**
  `settings_set_setting`, `settings_get_settings` (vorhanden, Beschreibungen ergänzen).
- **Abnahme:** Handler-Tests (gültig, unbekannter Befehl, gesperrte Taste, Doppelbelegung auch mit Einzeltaste,
  `replace`, `reset`), Vorlagenwechsel behält eigene Tasten, „Endgültig löschen“ auf eigener Taste fragt erst,
  Freigabe-Text lesbar; Tests je Vorlage in `keys`; API-Referenz neu erzeugt.

### E10.3 Einstellungsseite „Keyboard shortcuts“ des Mail-Fensters
- [ ] offen – wartet auf S.20
- **Modell:** Sonnet · **Braucht:** E10.2, S.20 · **Dateien:** `crates/torro-settings-ui/src/mail.rs` (neuer Abschnitt
  „Keyboard shortcuts“), `crates/torroinbox/src/app.rs`
- **Oberfläche:** Auswahl „Preset“ (Torro, Apple Mail, Outlook, Thunderbird, Gmail). Darunter die Befehle mit ihren
  Tasten, eigene hervorgehoben. Klick auf die Taste → „Press keys…“ nimmt das Kürzel auf, Esc bricht ab. Doppelbelegung →
  Hinweis „Already used by {command}“ mit „Replace“ (`replace: true`). „Reset“ je Zeile und „Reset all“
  (`reset: "all"`), je ein Request. Skill `torro-design`.
- **API/MCP:** wie E10.2, kein neuer Request.
- **Abnahme:** Sichtprüfung Breit/Schmal, hell/dunkel; Tests der Nachrichten.


## E12 Kategorien eingehender Mail (Issue #221)

Konzept: [kategorien.md](../kategorien.md). Jede Mail bekommt zusätzlich zur Spur eine **Art**:
Rechnung, Sendung, Werbung, Anmeldecode, Betrugsversuch, Anfrage, Auftrag … Erkannt zuerst aus
festen Merkmalen, sonst von einem **lokalen Entscheidermodell** über die SystemOne-/Jev-Schnittstelle
(Ollama ≥ 0.35, `POST <base>/systemone`, Fähigkeit `decision`).

Entschieden am 2026-10-19 (Planung #221, Entscheidungen 1–7 im Issue):

1. Keine dritte Spur. Die Kategorie ist eine eigene Dimension neben „Für mich“/„Zur Info“; an
   Zählern, Tray-Zahl, Benachrichtigungen und gespeicherten Einträgen ändert sich nichts.
2. **Eine `choice`-Frage mit den 20 vom Modell entschiedenen Kategorien**, ein Aufruf je Mail, dazu
   `needs_reply` (`noul`) und `urgency` (`score`). 20 ist die engste dokumentierte Obergrenze
   (Jev-Referenz); Ollama erlaubt 26. `invitation`, `bounce` und `auto_reply` folgen aus Kopfzeilen
   und sind keine Optionen der Frage.
3. Konfidenzschwelle **0,6, fest im Code**, keine Einstellung. Unter der Schwelle steht `other`.
   Der Wert ist **nicht an echten Mails gemessen**; nachjustiert wird er in einem eigenen Schritt.
4. **Die Modellwahl läuft über Zuordnungen je Funktion** (`ai.list_assignments`,
   `ai.set_assignment`, `docs/ki.md`), neu gebaut mit genau einem Funktionsschlüssel
   `inbox.categorize` und offen für weitere. Verworfen: ein eigenes Feld in `Settings` wie
   `assistant_default` – dann hätte die Tabelle im Issue keinen `ai.*`-Weg und müsste später
   migriert werden.
5. **`Category` wohnt in `torro-mail`**, die übersetzten Beschriftungen in `torro-core`
   (`torro_core::category`). Grund: `torro-core` hängt von `torro-mail` ab, nicht umgekehrt – ein
   `tr()` in `torro-mail` wäre ein Abhängigkeitskreis. `torro-inbox` hängt von beiden ab.
6. **Icon und Farbe je Gruppe (vier), Beschriftung je Kategorie.** Verworfen: 23 Icons und 23
   Farben – das bräuchte rund 20 neue SVGs und eine Palette, die nicht mehr farbenblind-sicher
   bleibt.
7. **Gefiltert wird über ein getyptes Feld** `categories` in `inbox.list_items` und
   `mail.search_messages`, nicht über einen `category:`-Operator der Suchsprache: Eine Gruppe sind
   bis zu sieben Kategorien, und ein getyptes Feld ist exakt, ohne Parser-Arbeit und
   Cursor-verträglich.

Die Seitenleiste wird gegen den **heutigen** Stand geplant (Ordner, „With attachments“,
„Invitations“); E9.2 („All“/„Later“) und E9.3a/b (Bounces) sind offen. `bounce` steht im Katalog,
setzt aber niemand, bis E9.3a kommt. Nicht parallel: zwei Aufgaben in `crates/torroinbox`, zwei am
Katalog (A-, M-, G-, P-Aufgaben), zwei an `crates/torro-ipc`.

```
E12.1 ─┬─ E12.2 ─┬─ E12.4 ─┬─ E12.6 (braucht auch E12.3 und E12.5)
       └─ E12.3 ─┘         ├─ E12.5
                           └─ E12.8 (braucht auch E12.7) ── E12.9
E12.7 (sofort möglich) ────┘
E12.10 braucht E12.5 und E12.6
```

### E12.1 Modell: Kategorie, Gruppe, Herkunft
- [x] erledigt (2026-10-19): `torro_mail::category` mit `Category` (23), `CategoryGroup` (4),
  `CategorySource` und `MailCategory`; `torro_core::category` mit `label`, `group_label` und der
  englischen `description` fürs Modell; Felder an `MessageSummary` und `Item`. Zwei Abweichungen
  vom Plan: `MailCategory::confidence` ist `Option<u8>` in **Prozent** statt `Option<f32>`, weil
  `MessageSummary` und `Item` `Eq` ableiten und `f32` nicht `Eq` ist; und alle Beschriftungen
  laufen über `trc("category", …)` statt `tr`, weil fünf der englischen Wörter in der Suite schon
  etwas anderes heißen (`Task`, `Order`, `Event`, `Other`, `Junk`).
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-mail/src/{lib,category}.rs` (neu:
  `category.rs`), `crates/torro-mail/src/message.rs`, `crates/torro-core/src/{lib,category}.rs`
  (neu: `category.rs`), `crates/torro-core/src/i18n/de.rs`, `crates/torro-inbox/src/item.rs`
- **`torro_mail::category`** (Daten, ohne `tr()`, weil `torro-mail` kein `torro-core` kennt):
  `Category` mit den 23 Schlüsseln aus `docs/kategorien.md` (`#[serde(rename_all = "snake_case")]`,
  `Serialize`, `Deserialize`, `JsonSchema`, `Copy`, `Hash`, `PartialOrd`, `Ord`), `Category::ALL`
  in der Reihenfolge des Katalogs, `Category::key()`, `Category::group()`.
  `CategoryGroup` mit `WaitingForMe`, `PaperTrail`, `Info`, `Junk`, dazu `ALL` und `key()`.
  `CategoryGroup::categories()` gibt die Kategorien der Gruppe in Katalogreihenfolge.
  `Category::BY_MODEL`: die genau 20 Kategorien, die das Modell entscheidet – alle außer
  `Invitation`, `Bounce`, `AutoReply` –, in Katalogreihenfolge; dazu `Category::by_rule_only()`.
  `CategorySource` mit `Rule`, `Model`, `Person` (`snake_case`), und
  `MailCategory { category: Category, source: CategorySource, confidence: Option<f32> }`
  (`confidence` nur bei `Model`, `skip_serializing_if`).
- **`torro_core::category`** (Beschriftungen): `label(Category) -> &'static str` und
  `group_label(CategoryGroup) -> &'static str`, beide über `tr("…")` mit den englischen Texten aus
  `docs/kategorien.md`; `description(Category) -> &'static str` mit der Zeile „Was es ist“, die
  E12.3 als `criteria` der `choice`-Frage verschickt (**englisch, ohne `tr()`** – sie geht an ein
  Modell, nicht an einen Menschen). Deutsche Übersetzungen der Beschriftungen in `de.rs`.
- **Felder:** `MessageSummary::category: Option<MailCategory>` und
  `torro_inbox::Item::category: Option<MailCategory>`, beide
  `#[serde(default, skip_serializing_if = "Option::is_none")]`, mit Doc-Kommentar: nur Mail setzt
  sie, GitHub, Talk und ntfy lassen sie leer; eine Kategorie mit `source: model` ist ein Vorschlag.
  `torro_mail::Message` nimmt sie über sein `#[serde(flatten)]` von `MessageSummary` mit – nichts zu
  tun.
- **Nachzuziehen:** nachgesehen – alle Aufrufstellen bauen `MessageSummary` und `Item` über
  Struktur-Literale; die neuen Felder brauchen dort je eine Zeile `category: None`
  (`crates/torroworkplace/src/mail/store.rs`, `crates/torro-inbox/src/demo.rs`,
  `crates/torro-settings-ui/src/demo.rs`, `crates/torroinbox/src/*` und die Tests). Grep nach
  `MessageSummary {` und `Item {`.
- **Oberfläche:** keine. **API/MCP:** keine (die Felder kommen mit E12.4 in den Katalog).
- **Abnahme:** Tests in `category.rs`: jede Kategorie wird als ihr `key()` serialisiert und zurück
  gelesen; `Category::ALL` hat 23 Einträge, jede Gruppe ist nicht leer und die Gruppen zusammen
  ergeben `ALL` ohne Dublette; `Category::BY_MODEL` hat **genau 20** Einträge und enthält
  `Invitation`, `Bounce`, `AutoReply` nicht; jede Kategorie und jede Gruppe hat eine nicht leere
  Beschriftung und jede Kategorie eine nicht leere englische `description`. In `de.rs` ein Test,
  dass jede Beschriftung übersetzt ist (`TORRO_LANG=de`). Übersetzungen im Bericht.

### E12.2 Speicher und feste Regeln
- [x] erledigt (2026-10-19): Schema 7 mit `category`, `category_source`, `category_confidence`,
  `needs_reply`, `urgency` und Index; `rules::mail_category` und `rules::kept_category`;
  `RuleEffect::Category` samt Armen in `approvals.rs`, der Seite „Rules“ und `effect_column`;
  `MailStore::set_category` und der typisierte Filter `SearchQuery::categories`; die feste
  Kategorie wird beim Eintreffen gesetzt (`Inbox::mail_category_of`, `inbox_feed::file`).
  Abweichung: Der Filter sitzt in `SearchQuery`, nicht in `query.rs` – Entscheidung 7 wollte ein
  getyptes Feld statt eines Suchoperators, damit braucht die Suchsprache nichts. Nebenbefund
  behoben: Drei Abfragen lasen Spalten **nach** `SUMMARY_COLUMNS` über feste Indizes und brachen
  durch die drei neuen Spalten; sie zählen jetzt von `SUMMARY_WIDTH`.
- **Modell:** Opus · **Braucht:** E12.1 · **Dateien:**
  `crates/torroworkplace/src/mail/store.rs`, `crates/torroworkplace/src/mail/query.rs`,
  `crates/torroworkplace/src/mail/inbox_feed.rs`, `crates/torro-inbox/src/{item,rules}.rs`,
  `crates/torroworkplace/src/{inbox,approvals}.rs`, `crates/torro-settings-ui/src/inbox.rs`,
  `crates/torro-ipc/src/api.rs`, `docs/api-reference.md`
- **Schema 7** in `store.rs` neben `SCHEMA_V6`: `ALTER TABLE messages ADD COLUMN category TEXT;`,
  `… category_source TEXT;`, `… category_confidence REAL;`, `… needs_reply INTEGER;`,
  `… urgency INTEGER;` und
  `CREATE INDEX messages_by_category ON messages (category, date DESC);`. Die beiden letzten
  Spalten bleiben vorerst leer und werden von nichts gelesen; sie stehen hier, weil derselbe
  Modellaufruf sie ohnehin mitbeantwortet (E12.6) und ein späteres Nachrüsten hieße, das Modell
  über den ganzen Altbestand noch einmal laufen zu lassen. `SCHEMA_VERSION` auf 7,
  Migration in der vorhandenen `match`-Kette; Zeilen von vorher lesen als `None`. Lesen und
  Schreiben in denselben Funktionen, die heute `invite_json`/`invite_answer` tragen; eine Mail ohne
  Kategorie bleibt `NULL`.
- **Filter:** `query.rs` bekommt ein `Field`/`Test` für die Kategorie, damit `SearchQuery` nach
  einer Menge von Kategorien filtern kann (`category IN (…)`), kombinierbar mit allem anderen und
  vor dem Blättern angewandt, damit Seiten und `cursor` halten.
- **Feste Regeln** in `torro_inbox::rules`: `pub fn mail_category(…) -> Option<MailCategory>` neben
  `mail_lane`, in dieser Reihenfolge und immer mit `CategorySource::Rule`:
  1. eine gemerkte Regel für Absender oder Domain (unten),
  2. `Category::Invitation`, wenn die Mail ein `torro_mail::Invite` trägt,
  3. `Category::AutoReply`, wenn `Auto-Submitted` etwas anderes als `no` sagt
     (`MessageTraits::auto_submitted`, dieselbe Prüfung wie in `inbox_feed::bulk_reason`),
  4. `Category::Spam`, wenn die Mail im Ordner mit der Rolle `torro_mail::Role::Junk` liegt.
  `Category::Bounce` bleibt frei: der Erkenner kommt mit E9.3a. Keine Regel setzt eine Kategorie
  über eine schon gespeicherte mit `CategorySource::Person`.
- **Gemerkte Regeln:** `RuleEffect` bekommt die Variante `Category { category: Category }`, benutzt
  mit `Rule::sender` bzw. `Rule::domain` wie `inbox.hide_sender`. `rules::mutes`/`drops` bleiben
  unberührt – eine Kategorieregel verwirft nichts und ändert keine Spur; sie wird nur von
  `mail_category` gelesen. Die Variante **reicht weiter, als es aussieht** – überall, wo heute
  über `RuleEffect` entschieden wird, braucht sie einen Arm, sonst baut es nicht oder der Mensch
  liest einen Allerweltssatz:
  - `crates/torroworkplace/src/approvals.rs`, der `match` unter `Request::RemoveRule` (heute
    `Muted`, `Lane`, `NoPopup`) – ein Satz wie „Stop filing mail from … as Invoice“, den Absender
    und die Kategorie **aus `approvals::Facts`**, nie aus dem Text des Clients.
  - `crates/torro-settings-ui/src/inbox.rs`, Seite „Rules“: `ordered.sort_by_key` (Reihenfolge),
    `effect_words` (Beschriftung) und der Treffertext zu `RuleHits` – alle drei `match` über
    `RuleEffect`.
  - Die Beschreibungen von `inbox.list_rules` und `inbox.remove_rule` in
    `crates/torro-ipc/src/api.rs` zählen die Regelarten einzeln auf und bekommen die neue dazu;
    danach `docs/api-reference.md` neu erzeugen.
  Der Doc-Kommentar von `Rule` und `RuleEffect` nennt sie ebenfalls.
- **Beim Eintreffen** (`inbox_feed`, `Inbox::file_mail`): die feste Kategorie wird gleich gesetzt
  und mit der Mail gespeichert; der Eintrag bekommt sie an `Item::category`. Mail ohne feste
  Kategorie bleibt vorerst ohne – das Modell kommt mit E12.6.
- **Oberfläche:** die vorhandene Seite „Rules“ des Notification Hub zeigt die neue Regelart mit.
  **API:** keine neuen Befehle; `inbox.list_rules` und `inbox.remove_rule` bekommen die neue
  Regelart in Beschreibung und Schema. **MCP:** keine neuen.
- **Abnahme:** Test, dass eine Datenbank in Schema 6 auf 7 wandert und ihre Mails erhalten bleiben;
  Test je fester Regel (Einladung, Abwesenheit, Junk-Ordner, Absenderregel vor allem anderen,
  `Person` schlägt jede Regel); Test, dass der Filter nach Kategorien über Seiten hinweg hält;
  Test, dass die Freigabe zum Entfernen einer Kategorieregel Absender und Kategorie nennt und
  beide aus `Facts` kommen.
  Prüfbefehle aus `docs/issue-workflow.md` sauber.

### E12.3 SystemOne/Jev im KI-Anbieter
- [x] erledigt (2026-10-19): `torro_core::ai::systemone` mit `DecisionRequest`, `Question`,
  `DecisionReply`, `Answer`, `decide_body_for` und `read_decision`, Grenzen als Konstanten (die
  engeren der Jev-Referenz); `openai::ollama_decision`, Feld `AiModel::decision`;
  `Provider::decide` mit Vorgabe-Implementierung, gesetzt von `OpenAiCompatible` (nur Ollama) und
  durchgereicht von `AiClient`. `httpmock`-Test gegen `/v1/systemone`, kein echtes Netz.
- **Modell:** Opus · **Braucht:** E12.1 · **Dateien:**
  `crates/torro-core/src/ai/{mod,systemone,openai}.rs` (neu: `systemone.rs`),
  `crates/torroworkplace/src/account/ai.rs`
- **`torro_core::ai::systemone`** – das Drahtformat, rein und ohne Netz, wie `ai::openai`:
  `DecisionRequest { model, state, questions }`, `Question` mit `Choice { instructions, criteria:
  BTreeMap<String, String> }`, `Noul { instructions }`, `Score { instructions, criteria:
  Vec<String> }`; `DecisionReply { answers }` mit `Answer::Choice { choice, probabilities,
  confidence }`, `Answer::Noul { noul }` (eine **Wahrscheinlichkeit** 0–1, kein Wahrheitswert),
  `Answer::Score { score, confidence }` (eine **Zahl**, auch gebrochen). Dazu
  `decide_body_for(&DecisionRequest) -> serde_json::Value` und
  `read_decision(&[u8]) -> Result<DecisionReply, AiError>`; Fehler über das vorhandene
  `openai::error_of`, das keine Schlüssel und keine Serverworte weiterreicht.
  Grenzen als Konstanten, die engeren der Jev-Referenz: `QUESTIONS_MAX = 8`,
  `CHOICE_OPTIONS_MAX = 20`, `SCORE_LEVELS_MAX = 10`, `STATE_MAX = 8000` (Zeichen),
  `INSTRUCTIONS_MAX = 1800`. `decide_body_for` kürzt `state` auf `STATE_MAX` an einer
  Zeichengrenze; eine Anfrage über den anderen Grenzen ist ein `AiError::Invalid`, kein Panik-Fall.
- **`decision` als Fähigkeit:** `openai::ollama_decision(&[u8]) -> Option<bool>` neben
  `ollama_tools`/`ollama_thinking` (dieselbe Hilfsfunktion über `capabilities`);
  `AiModel::decision: Option<bool>` mit Doc-Kommentar (Ollama ≥ 0.35 nennt sie in `/api/show`;
  `null`, wo der Server nichts sagt). `OpenAiCompatible::ollama_capabilities` gibt sie mit zurück
  und `Provider::models` trägt sie ein.
- **`Provider::decide`:** neue Methode
  `fn decide<'a>(&'a self, request: &'a DecisionRequest) -> BoxFuture<'a, Result<DecisionReply, AiError>>`
  mit **Vorgabe-Implementierung**, die `AiError::Invalid` mit einem Satz für Menschen liefert, damit
  die vorhandenen Test-Implementierungen (`crate::suggest::tests::Fake`,
  `crate::assistant::model::tests::Scripted`) unverändert bauen und die Schnittstelle object-safe
  bleibt. `OpenAiCompatible` setzt sie: `POST <base>/systemone` (die Basis endet schon auf `/v1`),
  nur für `AiProvider::Ollama` – jeder andere Anbieter behält die Vorgabe, weil nur Ollama das
  Protokoll spricht. **Nachgetragen 2026-10-19 nach dem Rebase auf #218:** Es gibt inzwischen drei
  echte Implementierungen – `OpenAiCompatible`, `Anthropic` und der Verteiler `AiClient`. `Anthropic`
  behält die Vorgabe; `AiClient` **reicht `decide` an die gewählte Implementierung durch**, sonst
  käme der Aufruf nie bei Ollama an.
- **Nachzuziehen:** der Doc-Kommentar oben in `ai/mod.rs` nennt bisher nur Chat; er bekommt einen
  Absatz über Entscheidermodelle. `crates/torro-ipc` ist nicht betroffen (nachgesehen:
  `AiModel` wird dort nur weitergereicht) – das Feld erscheint mit E12.4 im Katalogtext.
- **Oberfläche:** keine. **API/MCP:** keine (`ai.list_models` bekommt das Feld in E12.4).
- **Abnahme:** Tests in `systemone.rs` gegen die Beispiele aus `docs/kategorien.md › Quellen`: eine
  Anfrage mit `choice`, `noul` und `score` ergibt den dokumentierten Rumpf; eine Antwort mit allen
  drei Antwortarten wird gelesen, eine unvollständige wird `AiError::Format`; `state` über
  `STATE_MAX` wird gekürzt, 21 Optionen sind `AiError::Invalid`. In `account/ai.rs` ein
  `httpmock`-Test, dass `decide` an `/v1/systemone` geht und die Antwort liest, und dass
  `ollama_decision` aus `capabilities` gelesen wird. Kein echtes Netz.

### E12.4 Requests, Katalog, MCP
- [x] erledigt (2026-10-19): die fünf Befehle mit Katalogeintrag, Zugriffsregel (`remember` →
  Admin) und je einem MCP-Tool (die entstehen aus dem Katalog, `mcp::request_for` braucht keinen
  eigenen Zweig); Filterfeld `categories` an `inbox.list_items` und `mail.search_messages` samt
  `ItemFilter` und `SearchQuery`; `category` in den Beschreibungen der vier Lesebefehle, mit dem
  Satz, dass `source: model` ein Vorschlag ist; Freigabe-Satz in `approvals.rs`, Protokollzeile in
  `audit.rs` (nur die Kennung, nie die Kategorie) und Beschriftung in `daemon.rs`.
  `inbox.classify_item` und `inbox.catch_up_categories` antworten bis E12.6 `not-connected`.
  Abweichung: `docs/api.md` hat keine Befehlstabelle für den Eingang – die Liste steht in der
  erzeugten `docs/api-reference.md`, die mitcommittet ist.
- **Modell:** Opus · **Braucht:** E12.2, E12.3 · **Dateien:**
  `crates/torro-ipc/src/{lib,api,inbox,mail}.rs`, `crates/torro-inbox/src/source.rs`,
  `crates/torroworkplace/src/{core,inbox,mcp,approvals,audit,daemon}.rs`,
  `crates/torroworkplace/src/mail/service.rs`, `docs/api.md`, `docs/api-reference.md`
- Skill `torro-api` laden. Neue Requests mit Katalogeintrag (`Request::operation` ist erschöpfend)
  und je einem MCP-Tool gleichen Namens mit `_` statt `.`:
  | Request | Befehl | Zugriff | MCP |
  |---|---|---|---|
  | `ListCategories` | `inbox.list_categories` | Read | `inbox_list_categories` |
  | `SetItemCategory` | `inbox.set_item_category` | Write, mit `remember: true` Admin | `inbox_set_item_category` |
  | `ClassifyItem` | `inbox.classify_item` | Write | `inbox_classify_item` |
  | `GetCategorization` | `inbox.get_categorization` | Read | `inbox_get_categorization` |
  | `CatchUpCategories` | `inbox.catch_up_categories` | **Admin** | `inbox_catch_up_categories` |

  `inbox.catch_up_categories` ist **Admin**, nicht Write: Ein Aufruf stößt bis zu 500 Modellaufrufe
  je Mailkonto an und belegt den Rechner auf Minuten – das gibt ein Mensch frei, wie bei
  `inbox.mute_source`. Gerechnet wird nur über die Konten, die der Schlüssel lesen darf.
- `inbox.list_categories { account }` gibt den Katalog: je Kategorie Schlüssel, Gruppe,
  Beschriftung, die Beschriftung der Gruppe und die Zahl der ungelesenen Mails. Die Zahlen zählen
  **nur Mail aus Konten, die der Schlüssel lesen darf** – Leserecht auf `mail` **und** das Konto in
  `Rights::mail_accounts`, genau wie `inbox.list_items` es für Mail-Einträge tut; ein Schlüssel mit
  Inbox-Read und ohne Mailrechte bekommt überall 0, nie eine Zahl über ein fremdes Postfach. Die
  Beschreibung schreibt das fest. Das optionale `account` (`mail:<adresse>`, wie bei
  `mail.list_folders`) grenzt auf ein Konto ein, damit die Zahl in der Seitenleiste und die Liste
  daneben nicht auseinanderlaufen. Keine Fremdtexte, also kein `untrusted`.
- `inbox.set_item_category { id, category, remember }`: `id` ist eine Eintragskennung oder eine
  Mailkennung (`m:…`), wie `inbox.mute_item` es schon kann. Setzt `CategorySource::Person` an Mail
  und Eintrag. `remember: true` legt eine `RuleEffect::Category`-Regel für den Absender an – eine
  dauerhafte Regel, also **Admin** und die Freigabe eines Menschen, wie bei
  `inbox.set_item_lane { remember }`; `Request::operation` entscheidet das am Feld. Antwort wie
  `inbox.hide_sender`: die Regel, ob sie neu ist, und die geänderten Kennungen. Eine Mail ohne
  Absenderadresse kann nicht gemerkt werden (`error`).
- `inbox.classify_item { id }`: lässt die Mail neu einordnen; löst einen Modellaufruf aus, daher
  Write. Ein **ausdrückliches** Neu-Einordnen **überschreibt auch eine Kategorie, die der Mensch von
  Hand gesetzt hat** – anders als der Lauf beim Eintreffen und der Altbestand, die eine Kategorie
  mit `CategorySource::Person` nie anfassen. Wer den Knopf drückt, will genau das.
  `not-connected`, wenn kein Entscheidermodell zugeordnet ist oder die Datenregel keines auf diesem
  Rechner übrig lässt – mit demselben Satz wie `crate::suggest::SuggestError::NoLocalAi`;
  `unsupported` für einen Eintrag ohne Mail (GitHub, Talk, ntfy), der keine Kategorie bekommt.
- `inbox.get_categorization`: an/aus, das zugeordnete Modell, und – **nur für die Konten, die der
  Schlüssel lesen darf**, wie bei `inbox.list_categories` – je Konto der Stand des
  Altbestands (`messages`, `done`, `state` mit `complete`, `running`, `cancelled`, `not_started`),
  nach dem Vorbild von `torro_ipc::mail::MailIndexProgress`/`IndexState`.
- `inbox.catch_up_categories { cancel: bool }`: startet das Nachrechnen des Altbestands oder bricht
  es ab.
- **Felder an vorhandenen Befehlen:** `category` kommt mit `Item` und `MessageSummary` aus E12.1
  automatisch in `inbox.list_items`, `inbox.get_item`, `mail.search_messages` und
  `mail.get_message`; die Beschreibungen dieser vier Katalogeinträge nennen es und sagen
  ausdrücklich, dass eine Kategorie mit `source: model` ein **Vorschlag mit Konfidenz** ist
  (`docs/ki.md` Grundsatz 5) – damit ein Agent sie nicht für eine Tatsache hält. Neues Filterfeld
  `categories: Option<Vec<Category>>` an `torro_inbox::ItemFilter` (also `inbox.list_items`) und an
  `Request::SearchMail`, beide optional und mit `skip_serializing_if`, Bedeutung „eine der
  genannten“. `ai.list_models` nennt in seiner Beschreibung das neue Feld `decision`.
- **Freigabe, Protokoll, Benachrichtigung** für `inbox.set_item_category { remember: true }` – ohne
  sie bekommt der Mensch einen Allerweltssatz statt der Sache, über die er entscheidet:
  - `approvals.rs`: ein eigener Arm in `inbox_summary` („Always file mail from … as Invoice“), den
    Absender über `torro_inbox::rules::mail_sender` **aus `approvals::Facts`**, genau wie der
    vorhandene Arm für `Request::SetItemLane { remember: true }`; ohne ihn fällt der Request in
    `other => summary(other, &Facts::default())`.
  - `audit.rs`, `ids`: die Kennung des Eintrags neben die von `MuteItem` – **nur die Kennung, nie
    die Kategorie** (dieselbe Regel wie „never the trait muted or its value“).
  - `daemon.rs`: eine Beschriftung für die Benachrichtigung, ebenfalls ohne die Kategorie.
- **Nachzuziehen:** `docs/api.md` (Tabelle des Bereichs `inbox`), `docs/api-reference.md` neu
  erzeugen mit `TORRO_BLESS=1 cargo test -p torro-ipc` und mitcommitten. Die Liste „Was nie nach
  außen geht“ bleibt unverändert: kein Befehl braucht `Never`.
- **Oberfläche:** keine (kommt mit E12.8 und E12.10). **API/MCP:** die Tabelle oben.
- **Abnahme:** der Tabellen-Test über alle Katalogeinträge und der Vertragstest „Tool ↔ Katalog“
  laufen; eigener Fall, dass `inbox.set_item_category` mit `remember: true` Admin braucht und ohne
  nur Write; Handler-Tests im Kern mit Temp-Verzeichnissen, ohne Netz; `not-found` für Konten, die
  der Schlüssel nicht sehen darf; ein Schlüssel ohne Mailrechte bekommt von `inbox.list_categories`
  und `inbox.get_categorization` keine Zahl über ein fremdes Postfach; die Freigabe zu
  `set_item_category { remember: true }` nennt Absender und Kategorie, und die Protokollzeile nennt
  die Kategorie **nicht**. Agent `torro-api-review` ohne offene Funde.

### E12.5 Zuordnung je Funktion: `ai.list_assignments` und `ai.set_assignment`
- [x] erledigt (2026-10-19): `torro_core::ai::AiFunction` (vorerst `inbox.categorize`) und
  `AiAssignment`, gespeichert in `Settings::ai_assignments`, nachsichtig gelesen; die beiden
  Befehle mit Katalogeintrag (Read bzw. Admin) und MCP-Tool; `ai.set_assignment` prüft Konto,
  Modell, `decision` und – solange `mail_to_cloud` aus ist – `Place::Local`. `docs/ki.md`
  nachgezogen.
- **Modell:** Opus · **Braucht:** E12.4 · **Dateien:**
  `crates/torro-core/src/{ai/mod,settings}.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torroworkplace/src/{core,mcp,settings_service}.rs`, `docs/api.md`,
  `docs/api-reference.md`, `docs/ki.md`
- Skill `torro-api` laden. Umsetzt den Abschnitt „Zuordnung je Funktion“ aus `docs/ki.md`, **mit
  genau einem Funktionsschlüssel** und offen für weitere.
- **Modell:** `torro_core::ai::AiFunction` mit vorerst einer Variante `InboxCategorize`
  (`serde`-Name `inbox.categorize`), dazu `ALL`, `key()` und `label()` über `tr()`.
  `AiAssignment { function: AiFunction, engine: String, model: Option<String>, effort:
  Option<Effort>, enabled: bool }` – dieselben Felder wie `settings::AssistantDefault`, das als
  Vorbild dient, plus `enabled` für „Kategorisierung an/aus“. Gespeichert in
  `Settings::ai_assignments: BTreeMap<AiFunction, AiAssignment>`, `#[serde(default,
  skip_serializing_if = "BTreeMap::is_empty", deserialize_with = …)]` und **nachsichtig gelesen**
  wie `assistant_default`: ein Wert, der nicht passt, fällt weg und lässt die Einstellungsdatei nie
  scheitern. Ein Funktionsschlüssel, den diese Fassung nicht kennt, wird beim Lesen verworfen.
- **Befehle:** `ai.list_assignments` (Read, MCP `ai_list_assignments`) listet je Funktion den
  Schlüssel, die Beschriftung, Konto, Modell, Aufwand und ob sie an ist – auch die Funktionen ohne
  Zuordnung, mit leeren Feldern, damit die Oberfläche die Liste nicht selbst kennt.
  `ai.set_assignment { function, engine, model, effort, enabled }` (**Admin**, MCP
  `ai_set_assignment`): setzt eine Zuordnung oder nimmt sie mit `engine: null` zurück. `not-found`
  für eine unbekannte Funktion oder ein Konto, das es nicht gibt; `error` für ein Modell, das das
  Konto nicht anbietet. Kein Geheimnis verlässt den Dienst – die Antwort nennt nur Kennungen.
- **Datenregel:** `ai.set_assignment` nimmt für `inbox.categorize` **kein Modell an, das nicht auf
  diesem Rechner rechnet**, solange `Settings::mail_to_cloud` aus ist (`data_rule::place_of`,
  `Place::Local`); die Antwort ist `error` mit dem Satz aus
  `crate::suggest::SuggestError::NoLocalAi`. So kann auch ein Agent die Regel nicht umgehen.
- **Nachzuziehen:** `docs/ki.md` – der Abschnitt „Zuordnung je Funktion“ bekommt eine Zeile, dass
  die Befehle jetzt stehen und welcher Funktionsschlüssel es gibt. `docs/api.md` (Bereich `ai`),
  `docs/api-reference.md` neu erzeugen.
- **Oberfläche:** keine (kommt mit E12.10). **API/MCP:** die beiden Befehle oben.
- **Abnahme:** Tabellen- und Vertragstest laufen; Test, dass `ai.set_assignment` Admin verlangt;
  Test, dass ein Modell mit `remote: true` für `inbox.categorize` abgelehnt wird, solange
  `mail_to_cloud` aus ist, und angenommen, wenn er an ist; Test, dass eine Einstellungsdatei mit
  einem unbekannten Funktionsschlüssel gelesen wird, ohne zu scheitern. Agent `torro-api-review`
  ohne offene Funde.

### E12.6 Einordnen mit dem Modell: beim Eintreffen und Altbestand
- [x] erledigt (2026-10-19): `crate::categorize` mit `Content`, `request` (drei Fragen, 20
  Optionen), `read` (Schwelle 0,6, `other` darunter), `chosen` und `pick_model`;
  `MailStore::{set_model_answers, uncategorised}`, `MailService::{content_for_category,
  uncategorised, accounts_in, set_model_answers}`; `inbox.classify_item` überschreibt auch eine
  Kategorie des Menschen, der Altbestandslauf und das Einordnen beim Eintreffen nie. Der Lauf
  hängt an `Core::catch_up` (ein Lauf zur Zeit, 250 ms Pause, Abbruch nach dem laufenden Aufruf);
  der Feed bekommt den Modellaufruf als `inbox_feed::Decider` hereingereicht, damit er von
  KI-Konten, Zuordnungen und Datenregel nichts wissen muss. Fixtures unter
  `crates/torroworkplace/tests/fixtures/categories/`.
  Nach dem Korrektheits-Review: Neue Mail wird **erst abgelegt, dann gefragt** – der Modellaufruf
  läuft in einer eigenen Aufgabe mit Zeitgrenze (60 s) und Pause (250 ms), und die Kategorie
  kommt danach auch auf den Eintrag; vorher hing jede Zustellung an einer Reihe ungedrosselter
  Aufrufe und der Eintrag blieb ohne Kategorie. Absender und Mailingliste stehen jetzt **im**
  `<message>`-Zaun und gehen durch `fenced()`.
  Abweichung: Keine eigene Protokollzeile je Modellaufruf – `audit.rs` hat dafür kein Format, und
  die Zeile des auslösenden Requests steht schon ohne Inhalte darin.
- **Modell:** Opus · **Braucht:** E12.3, E12.4, E12.5 · **Dateien:**
  `crates/torroworkplace/src/categorize.rs` (neu), `crates/torroworkplace/src/main.rs` (Modul),
  `crates/torroworkplace/src/mail/inbox_feed.rs`, `crates/torroworkplace/src/mail/store.rs`
  (`set_model_answers` und `uncategorised` – in E12.2 wären sie bis hierher toter Code),
  `crates/torroworkplace/src/{core,inbox}.rs`,
  `crates/torroworkplace/src/audit.rs`, `crates/torroworkplace/tests/fixtures/` (neu)
- **`crate::categorize`** nach dem Vorbild von `crate::suggest` (derselbe Aufbau: Inhalt lesen,
  Modell wählen, einmal fragen, Antwort lesen; nichts wird selbst geschrieben):
  - `Content` – Kopfzeilen und der Anfang der Mail. Der Text wird auf `systemone::STATE_MAX`
    gekürzt und steht zwischen `<message>` und `</message>`, eine schließende Marke darin wird
    aufgebrochen, genau wie in `suggest::messages`. Was hereinkommt, ist Fremdtext und wird nie als
    Anweisung gelesen.
  - `questions()` – die drei Fragen aus `docs/kategorien.md › Die drei Fragen`: `category`
    (`choice` über `Category::BY_MODEL`, `criteria` aus `torro_core::category::description`),
    `needs_reply` (`noul`), `urgency` (`score` mit `["routine", "soon", "urgent"]`).
  - `const CONFIDENCE_MIN: f32 = 0.6;` mit Doc-Kommentar, dass der Wert nicht gemessen ist.
  - `read(reply)` – `Answer::Choice` unter `CONFIDENCE_MIN` wird `Category::Other`; `noul` ≥ 0,5
    heißt ja; `score` wird auf die nächste Stufe gerundet. Eine Antwort, die keine Frage
    beantwortet, ist ein Fehler, kein stiller Rückfall. Alle drei Antworten werden gespeichert:
    die Kategorie in `category`/`category_source`/`category_confidence`, die beiden anderen in
    `needs_reply` und `urgency` (Schema 7 aus E12.2). Gelesen werden die beiden letzten heute von
    niemandem – sie stehen da, damit ein späterer Schritt nicht das Modell über den ganzen
    Altbestand noch einmal laufen lassen muss.
  - `choose()` – das Konto und Modell aus `ai.list_assignments` für `AiFunction::InboxCategorize`.
    Es wird **gar nicht gefragt**, wenn keines zugeordnet ist, die Zuordnung aus ist, **das
    zugeordnete Konto nicht mehr besteht** (entfernt oder abgemeldet, seit es gesetzt wurde – die
    Zuordnung wird beim Setzen geprüft, kann aber später ins Leere zeigen) oder das Modell nicht
    `Place::Local` ist, während `mail_to_cloud` aus ist. Die Mail behält dann ihre feste Kategorie
    oder keine; es gibt keinen Fehler je Mail und keinen Eintrag im Eingang.
- **Wessen Kategorie gilt:** Der Lauf beim Eintreffen und der Altbestand fassen eine Kategorie mit
  `CategorySource::Person` **nie** an. Nur das ausdrückliche `inbox.classify_item` überschreibt
  auch sie (E12.4) – wer den Knopf drückt, will genau das.
- **Beim Eintreffen:** `inbox_feed` ruft nach der festen Regel aus E12.2 das Modell, wenn die Mail
  danach keine Kategorie hat; das Ergebnis geht mit `CategorySource::Model` an Mail und Eintrag.
  Scheitert der Aufruf, bleibt die Mail ohne Kategorie – nie `other`, damit ein späterer Lauf sie
  noch einholt.
- **Altbestand:** ein eigener Lauf im Hintergrund, gestartet und abgebrochen über
  `inbox.catch_up_categories`. Er sieht sich **nur Mail ohne Kategorie** an (die Spalte `category`
  ist `NULL`), damit ein zweiter Druck auf „Catch up now“ nicht alles noch einmal fragt. Je Konto
  höchstens die letzten **30 Tage** oder **500 Nachrichten**, neueste zuerst; gedrosselt mit einer Pause zwischen den Aufrufen, damit ein lokales Modell die
  Maschine nicht belegt. Der Stand (`messages`, `done`, `state`) ist das, was
  `inbox.get_categorization` meldet. Ein Abbruch hält nach dem laufenden Aufruf an und lässt das
  schon Eingeordnete stehen. Beim Neustart des Dienstes läuft nichts von selbst wieder an.
- **Protokoll:** `audit.rs` bekommt eine Zeile je Aufruf mit Funktion, Konto, Modell, Tokens und
  Dauer – **kein** Betreff, Absender oder Text (Sicherheitsregel 7 des Eingangs).
- **Oberfläche:** keine (kommt mit E12.8 und E12.10). **API/MCP:** keine neuen; `inbox.classify_item`
  und `inbox.catch_up_categories` aus E12.4 bekommen hier ihre Wirkung.
- **Abnahme:** Tests mit einem erfundenen `Provider` (kein Netz, wie `suggest::tests::Fake`): eine
  sichere Antwort ergibt ihre Kategorie, eine unter 0,6 ergibt `other`, ein Fehler lässt die Mail
  ohne Kategorie; `needs_reply` und `urgency` werden richtig gelesen; eine Mail mit
  `CategorySource::Person` wird vom Lauf beim Eintreffen und vom Altbestand nicht angefasst, von
  `inbox.classify_item` aber überschrieben; `needs_reply` und `urgency` landen in ihren Spalten;
  die Grenzen 30 Tage und 500 Nachrichten halten; ein Abbruch hält an; eine Zuordnung auf ein Konto, das es nicht mehr gibt, fragt nichts
  und meldet keinen Fehler; ein zweiter Lauf fragt nichts, was schon eine Kategorie hat. Fixtures mit mindestens zwei echten, anonymisierten Mail-Köpfen je
  Gruppe unter `crates/torroworkplace/tests/fixtures/`, damit die Schwelle später daran
  nachjustiert werden kann. Ein Test, dass keine Protokollzeile Betreff, Absender oder Text enthält.

### E12.7 Bausteine: Kategorie-Pille und Gruppenfarben
- [x] erledigt (2026-10-19): `Palette::category` (vier Gruppenfarben, hell und dunkel, in
  `design.rs`, `theme.rs` und `legacy.rs`), geprüft von `design/rules.rs` auf ≥ 3:1 gegen `panel`
  – die Regel fing sofort eine zu dunkle Müll-Farbe im Paket `nord`; `widgets::category_pill` mit
  Gruppen-Icon, Gruppenfarbe und Sparkles für einen Vorschlag; Galerie `--categories`;
  `docs/design-pakete.md` und der Skill `torro-design` nachgezogen. `scripts/design-check.sh`
  sauber.
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-gui/src/{theme,widgets}.rs`,
  `crates/torro-core/src/design.rs`, `crates/torro-core/src/design/rules.rs`,
  `crates/torro-gui/examples/gallery.rs`, `docs/design-pakete.md`,
  `.claude/skills/torro-design/SKILL.md`
- Skill `torro-design` laden. Darf sofort starten und läuft neben allem Übrigen – sie fasst
  `crates/torroinbox` nicht an.
- **Farben:** `Palette::category: [Color; 4]` – eine Farbe je Gruppe, in der Reihenfolge von
  `CategoryGroup::ALL` (Wartet auf mich, Belege, Info, Müll). Als Satz auf Farbenblindheit geprüft
  wie `Palette::chart`, hell und dunkel; nur als Punkt und Rand benutzt, nie als Textfarbe.
  Eintrag in `design/rules.rs` und in der Tabelle in `docs/design-pakete.md`; ein Paket ohne die
  Farben nimmt die des Standardpakets (Test).
- **Baustein** `widgets::category_pill(group_icon, group, label, suggested)` neben `pill` und
  `icon_pill`: `PILL_HEIGHT` hoch, rund, Rand `line_strong`, Icon 12 in `palette.category[group]`,
  Beschriftung `typo::caption()` in `text_2`. Mit `suggested` (eine Kategorie vom Modell,
  `docs/ki.md` Grundsatz 5) steht `Icon::Sparkles` in `accent` davor – dasselbe Zeichen, mit dem
  die Suite KI-Ergebnisse schon überall kennzeichnet. Keine Zahlen in der App: Maße und Farben nur
  über `theme`/`typo`/`layout`.
- **Icons je Gruppe**, alle vorhanden, kein neues SVG: `Icon::Inbox` (Wartet auf mich),
  `Icon::Archive` (Belege), `Icon::Info` (Info), `Icon::ShieldAlert` (Müll).
- **Galerie:** `--categories` zeigt alle vier Gruppen, mit und ohne Vorschlags-Zeichen, hell und
  dunkel, in `torro` und `plex`.
- **Skill `torro-design`:** Abschnitt 5 bekommt einen Eintrag „Kategorie-Pille“ mit der Regel
  „Farbe je Gruppe, Beschriftung je Kategorie, Vorschlag mit Sparkles“.
- **Oberfläche:** nur Bausteine. **API/MCP:** keine.
- **Abnahme:** Tests in `widgets.rs`/`theme.rs`: die vier Farben lassen sich als Satz
  unterscheiden, der Rand der Pille erreicht ≥ 3:1 auf `panel` und der Text ≥ 4.5:1, hell und
  dunkel; ein Paket ohne `category` nimmt den Standard. Galerie in `torro` und `plex`, hell und
  dunkel angesehen (Bilder im Bericht). `scripts/design-check.sh` sauber. Übersetzungen im Bericht.

### E12.8 Mail-Fenster: Pille, Seitenleiste, Kategorie ändern
- [x] erledigt (2026-10-19): Pille im Leser-Kopf, im Paneel und auf der Karte; in der Zeile das
  Gruppen-Icon in der Gruppenfarbe mit der Kategorie als Tooltip (Abweichung: die Marken einer
  Zeile sind laut ihrem Doc-Kommentar kleine Icons, eine 90-px-Pille sprengt Mittel und Schmal);
  Seitenleisten-Gruppe „Categories" mit vier aufklappbaren Gruppen in ihren Farben; Menü
  „Category…" nach Gruppen gegliedert und scrollbar, danach „Always for this sender?"; Knopf
  „Re-categorize" nur mit zugeordnetem Modell; `MailSource` um vier Methoden erweitert, Demo-Mail
  mit Kategorien aus allen vier Gruppen.
  **Sichtprüfung steht aus:** Der Agent `torro-design-review` konnte auf dieser Maschine keine
  Screenshots machen (macOS ohne Hyprland, `screencapture` verweigert den Bildschirmzugriff). Sein
  Urteil kam aus Code und Kontrastrechnung; acht Muss-Funde sind behoben (Pille im Leser fehlte,
  23-Zeilen-Menü ohne Gruppen und ohne Scrollen, unerreichbare Untereinträge, „Re-categorize" ohne
  Modell, Icon-Kollision mit den Ordner-Icons, fehlender Eintrag im Zeilenmenü, zerschnittene
  Doc-Kommentare, Demo ohne Beleg-Gruppe). Offen aus dem Review: kein Lade-Zustand während des
  Modellaufrufs, das Menü hängt am Lane-Anker statt an der Pille, und die Zahl der Gruppe zählt
  Ungelesene, während die Liste alle zeigt.
  Nach dem Korrektheits-Review: Die Seitenleiste zeigt eine Kategorie, **solange sie vorkommt**
  (neues Feld `messages` in `CategoryInfo`), nicht nur mit Ungelesenen; die Gruppe „Categories"
  fehlt ganz, wenn nichts eingeordnet ist. Gezählt und gelistet wird nur der Posteingang
  (`SearchQuery::in_inbox`) – Papierkorb, Spam und Gesendet tragen ihre Kategorie weiter, gehören
  aber unter keine. `inbox.set_item_category` und `inbox.classify_item` nehmen jetzt wirklich ein
  `m:…` für Mail ohne Eintrag, wie der Katalog es verspricht.
- **Modell:** Opus · **Braucht:** E12.4, E12.7 · **Dateien:**
  `crates/torroinbox/src/{app,list,mail,detail}.rs`, `crates/torro-core/src/i18n/de.rs`
- Skill `torro-design` laden. Nicht parallel zu E9.2, E9.3b oder E12.9.
- **Pille:** `widgets::category_pill` in der Zeile der Liste und im Kopf des Lesers, nach den
  Bausteinen aus E11.1/E11.3. Keine Kategorie, keine Pille.
- **Seitenleiste:** eine neue Gruppe **„Categories“** zusätzlich zu den vorhandenen Einträgen
  (Ordner, „With attachments“, „Invitations“) – `View` bekommt dafür
  `Category(CategoryFilter)` mit einer Gruppe **oder** einer Kategorie. Vier Einträge, je Gruppe
  einer, mit der Zahl der ungelesenen Mails; darunter aufklappbar die einzelnen Kategorien der
  Gruppe, **nur die, die im Postfach vorkommen**. Die ganze Gruppe erscheint erst, wenn mindestens
  eine Mail eine Kategorie hat. Zahlen und Vorkommen kommen aus `inbox.list_categories`, die Liste
  selbst aus `mail.search_messages` mit dem Feld `categories` – das Fenster rechnet nichts selbst.
- **Ändern:** im Menü der Zeile und im Leser ein Eintrag „Category ▸“ mit den Kategorien, nach
  Gruppen geordnet, aus `inbox.list_categories`. Nach der Wahl dieselbe Rückfrage wie beim
  Umsortieren der Spur („Always for this sender?“), die bei Ja
  `inbox.set_item_category { remember: true }` schickt; die Regel braucht eine Freigabe, das Fenster
  zeigt sie wie bei `inbox.set_item_lane`. Dazu ein Knopf „Re-categorize“ im Leser
  (`inbox.classify_item`), bei den KI-Knöpfen und nur sichtbar, wenn ein Modell zugeordnet ist.
- **Oberfläche:** Liste, Leser, Paneel und Seitenleiste des Mail-Fensters in allen Größenklassen.
  **API:** `inbox.list_categories`, `inbox.set_item_category`, `inbox.classify_item`,
  `inbox.get_categorization` (sagt, ob ein Modell zugeordnet ist – daran hängt, ob „Re-categorize“
  überhaupt erscheint; das Fenster findet das nie selbst heraus) und
  `mail.search_messages { categories }` – alle aus E12.4, keine neuen. **MCP:** keine neuen.
- **Abnahme:** Tests: die Gruppe „Categories“ fehlt, solange keine Mail eine Kategorie hat, und
  zeigt nur vorkommende Kategorien; die Wahl im Menü schickt `inbox.set_item_category` und bei Ja
  mit `remember: true`; eine Kategorie vom Modell trägt das Vorschlags-Zeichen, eine aus einer Regel
  nicht. Oberflächentexte in Tests über `tr("…")` vergleichen. Sichtprüfung mit Agent
  `torro-design-review` in Breit, Mittel und Schmal, hell und dunkel, `torro` und `plex`
  (Bilder im Bericht). Übersetzungen im Bericht.

### E12.9 Phishing: Band im Leser und entschärfte Links
- [x] erledigt (2026-10-19): `widgets::warning_band` (Schild in der Fehlerfarbe, getönte Fläche)
  über der Nachricht, nur bei `Category::Phishing`; beide Link-Wege (`Message::HtmlLink` und
  `Message::OpenMailLink`) gehen über `App::open_link` und halten den Link zurück, die Rückfrage
  nennt die **ganze** Adresse im Klartext, erst „Open anyway" öffnet. Die Schema-Prüfung
  (`javascript:`, `file:`) bleibt davor. Entscheidung dabei: Es reicht, wenn **eine** der beiden
  Quellen (geladene Mail oder Zeile der Liste) Betrug sagt – einmal zu viel fragen kostet einen
  Klick, einen Betrugslink einmal zu viel öffnen kostet mehr. Versteckt, verschoben oder gelöscht
  wird nichts.
  **Sichtprüfung steht aus**, aus demselben Grund wie bei E12.8 (keine Screenshots auf dieser
  Maschine).
- **Modell:** Opus · **Braucht:** E12.8 · **Dateien:**
  `crates/torroinbox/src/{app,mail}.rs`, `crates/torro-core/src/i18n/de.rs`
- Skill `torro-design` laden. Nicht parallel zu E12.8.
- **Band:** oben in der Nachricht, über dem Inhalt, in den Farben für Fehler (`palette.error`,
  `error_text`), mit `Icon::ShieldAlert` und einem Satz, der sagt, was der Verdacht ist und dass
  nichts verschoben oder gelöscht wurde. Nur bei `Category::Phishing`.
- **Entschärfte Links:** Es sind **genau zwei** Stellen, an denen das Mail-Fenster einen Link aus
  einer Nachricht öffnet (nachgesehen, beide in `app.rs` und beide über `App::open_url`):
  `Message::HtmlLink` für den HTML-Leser (mit `mail::opens_in_browser` davor) und
  `Message::OpenMailLink` für die Textansicht (Links aus `mail::pieces`, davor die Prüfung auf
  `http://`/`https://`). `Link::Url` in `App::open_entry` gehört einem Eintrag, nicht dem Text
  einer Mail, und bleibt unberührt. Beide Stellen öffnen bei einer Mail mit `Category::Phishing`
  nicht mehr sofort, sondern zeigen **dieselbe** Bestätigung mit dem **Ziel im Klartext** (die
  volle URL, nicht der Linktext) und den Knöpfen „Open anyway“ und „Cancel“; erst „Open anyway“
  ruft `open_url`. Versteckt, verschoben und gelöscht wird nichts; der einzige Eingriff ist der
  zusätzliche Klick.
- **Grenze der Änderung:** nur das Mail-Fenster und nur Mail mit dieser Kategorie. Die Zentrale,
  der App Feed und `torro-htmlview` bleiben unberührt; die Nachricht selbst wird nicht umgeschrieben.
- **Oberfläche:** Leser des Mail-Fensters. **API/MCP:** keine.
- **Abnahme:** Tests: das Band steht nur bei `phishing`; ein Klick auf einen Link einer
  Phishing-Mail öffnet nichts, sondern fragt, und erst die Bestätigung schickt `Sent::Link` mit
  genau der URL – je ein Fall für `Message::HtmlLink` und für `Message::OpenMailLink`; bei jeder
  anderen Mail öffnet der Klick wie bisher; die Prüfung der Schemata (`javascript:`, `file:`)
  bleibt davor. Oberflächentexte über `tr("…")` vergleichen. Sichtprüfung
  mit Agent `torro-design-review`, hell und dunkel. Übersetzungen im Bericht.

### E12.10 Einstellungen › Categorization
- [x] erledigt (2026-10-19): dritte Sektion `categorization` in `settings_mail::SECTIONS` mit
  an/aus, Modellwahl (nur Modelle mit `decision`, und solange `mail_to_cloud` aus ist keines mit
  `remote`, mit ruhiger Zeile dazu), Altbestand mit Stand und „Catch up now"/„Cancel", je
  Kategorie ein Schalter für die Seitenleiste (`Settings::hidden_categories`, neuer `SettingKey`)
  und ein Verweis auf die Regeln. Keine Einstellung für Zeitpunkt oder Konfidenzschwelle.
  Abweichung: Der Verweis auf „Rules" **öffnet** den Notification Hub nicht – `Action` dieser
  Bibliothek kennt kein „öffne diese App", und sie soll kein Fenster von sich aus aufmachen. Die
  Zeile sagt stattdessen, wo die Regeln stehen.
  Nach dem Review: Die Modellwahl ist eine eigene Gruppe „Model" mit einer Zeile je Modell (wie
  „New chats" im Assistenten), gespeist aus **allen** KI-Konten – `Data::follow_up` fragt
  `ai.list_models` je KI-Konto, und `Response::AiModels` nennt jetzt das Konto, damit die
  Antworten auseinanderzuhalten sind. Vorher war die Liste immer leer und die Funktion damit
  unerreichbar. Der Schalter „an" ohne Modell nimmt das erste, das entscheidet.
  **Sichtprüfung steht aus**, wie bei E12.8 und E12.9 (keine Screenshots auf dieser Maschine).
- **Modell:** Opus · **Braucht:** E12.5, E12.6 · **Dateien:**
  `crates/torro-settings-ui/src/{lib,mail,demo}.rs`,
  `crates/torro-settings-ui/src/mail/categorization.rs` (neu),
  `crates/torro-core/src/settings.rs`, `crates/torro-ipc/src/{lib,api}.rs` (`SettingKey` und die
  Beschreibungen von `settings.set_setting` und `settings.get_settings`, die die Schlüssel einzeln
  aufzählen), `crates/torroworkplace/src/settings_service.rs`, `crates/torroinbox/src/app.rs`,
  `crates/torro-core/src/i18n/de.rs`, `docs/api-reference.md`
- Skill `torro-design` laden. **Korrektur zum Issue:** Die Einstellungen des Mail-Fensters haben
  heute nur „Accounts“ und „Reading“ (`settings_mail::SECTIONS`); die Seite „Rules“ gehört zum
  Notification Hub (`settings_inbox::SECTIONS`, `Kind::settings_sections`). Die neue Seite kommt
  also zu `settings_mail::SECTIONS` dazu, und der Verweis auf „Rules“ führt in das andere Fenster.
- **Neue Sektion** `Section { id: "categorization", icon: Icon::Tag, label: "Categorization" }` in
  `settings_mail::SECTIONS`, dazu ein Modul in der Form der anderen Sektionen (`State`, `Message`,
  `update`, `view`). `settings_mail::wanted()` holt zusätzlich `ai.list_assignments`,
  `inbox.get_categorization` und `accounts.list_accounts`; `Data` bekommt die Felder dafür, und die
  Modelle des gewählten Kontos kommen über `Data::follow_up` wie `default_models` schon heute.
- **Zeilen der Seite**, in dieser Reihenfolge:
  1. **Kategorisierung an/aus** – `ai.set_assignment { function: "inbox.categorize", enabled }`.
  2. **Modell** – Auswahl aus den angeschalteten Modellen mit `decision: true`
     (`ai.list_models`), gruppiert nach Konto, lokale zuerst. Modelle mit `remote: true` erscheinen
     **gar nicht**, solange `mail_to_cloud` aus ist, mit einer ruhigen Zeile darunter, die das
     erklärt – kein Warnton, keine Aufforderung. Gewählt mit `ai.set_assignment`.
  3. **Altbestand nachrechnen** – Zustand aus `inbox.get_categorization` („Done“, „847 of 1,240“,
     „Not started“), Knopf „Catch up now“ und „Cancel“ (`inbox.catch_up_categories`), dazu die
     Zeile, wie weit zurück gerechnet wird (30 Tage oder 500 Nachrichten je Konto).
  4. **Je Kategorie: in der Seitenleiste zeigen** – ja/nein, gespeichert als
     `Settings::hidden_categories` über `settings.set_setting` (neuer `SettingKey` und `Change`,
     nach dem Vorbild von `hidden_collections`). Die Seitenleiste aus E12.8 liest es.
  5. **Verweis auf „Rules“** für die gemerkten Absender- und Domainregeln: eine Zeile, die den
     Notification Hub holt – genau wie `app::rules_message(Kind::Mail)` es für „All rules“ im
     Glockenmenü schon tut (`Message::OpenApp` mit `AppId::INBOX.window_class`, also `apps.open_app`,
     das den Hub startet, wenn keiner läuft).
  Keine Einstellung für Zeitpunkt und Konfidenzschwelle – beides steht fest.
- **Nachzuziehen:** `torro_settings_ui::demo::Demo` antwortet auf die neuen Requests, damit
  `--demo` die Seite zeigt, ohne einen Socket zu öffnen.
- **Oberfläche:** Einstellungen des Mail-Fensters, neue Seite „Categorization“.
  **API:** `ai.list_assignments`, `ai.set_assignment`, `ai.list_models`,
  `inbox.get_categorization`, `inbox.catch_up_categories` – alle aus E12.4/E12.5 – und
  `settings.set_setting` mit dem **neuen Schlüssel** `hidden_categories`, der also in `SettingKey`,
  in den Beschreibungen von `settings.set_setting` und `settings.get_settings` und danach in
  `docs/api-reference.md` auftaucht. **MCP:** keine neuen.
- **Abnahme:** Tests: die Sektion steht in `settings_mail::SECTIONS` und `--settings categorization`
  landet dort; nur Modelle mit `decision: true` stehen zur Wahl, und mit `mail_to_cloud: false`
  keines mit `remote: true`; „Catch up now“ schickt `inbox.catch_up_categories { cancel: false }`,
  „Cancel“ mit `true`; `hidden_categories` wird über `settings.set_setting` gesetzt und gelesen. Oberflächentexte über `tr("…")` vergleichen. Sichtprüfung mit Agent
  `torro-design-review` in allen Größenklassen, hell und dunkel, auch mit `--demo`
  (Bilder im Bericht). Übersetzungen im Bericht.

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
