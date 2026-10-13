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
- [ ] erledigt
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
- [ ] erledigt
- **Modell:** Opus · **Braucht:** E8.3, E8.7a · **Dateien:** `crates/torro-mail/src/` (Zusammenfassung einer
  Nachricht), `crates/torroworkplace/src/mail/{read,store,write}.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
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
- [ ] erledigt
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
- [ ] erledigt
- **Modell:** Opus · **Braucht:** E8.6a, E8.3, E8.4 · **Dateien:** `crates/torroinbox/src/{app,notifications,list,keys}.rs`,
  `crates/torroworkplace/src/ui/inbox.rs`, `crates/torro-gui/src/widgets.rs`, `crates/torro-core/src/i18n/de.rs`
- **Verwerfen** (✕, „Dismiss all“) schickt ab jetzt `set_item_done` mit `dismissed: true`.
- **Menü der Glocke** (und Taste `m`): Abschnitt „Mute“ mit den Möglichkeiten aus `list_mute_options`, „For“ (1 hour ·
  Until tomorrow · Always), „What happens“ (No popup · Only Info · Hide), unten der Weg zu den Regeln. Nach dem
  Stummschalten der Toast mit Undo (E3.11). Im Mail-Fenster bleibt „Hide from inbox“ (E3.10), es nutzt dieselben
  Regeln.
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
- [ ] erledigt
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

## Danach (Überblick, Aufgaben folgen)

- **E4 Mail als Quelle**, nach dem Mail-Kern (M2/M3 in [mail.md](../mail.md)); dort entscheiden: Spur
  „Belege“, „erledigt“ archiviert oder nicht.
- **E5 Nextcloud, GitHub, ntfy** als Quellen.
- **E6 Ruhe und Ordnung:** Screener, Lernen, Zusammenführen über Quellen; mit [ki.md](../ki.md) Spur
  vorschlagen und Zusammenfassungen.
- **E7 Slack**, danach Messenger (zweiter Schritt).
- **An einen Agenten geben** (vorgemerkt 2026-10-13): Ablauf, Rechte, Rückweg des Ergebnisses und Aussehen werden
  eigens ausgearbeitet, bevor es Aufgaben gibt ([eingang.md › Später](../eingang.md#später-vorgemerkt)).
- **Fokus-Profile** (vorgemerkt 2026-10-13): Profile wie der Fokus bei Apple, im Anschluss an E8.6 (gleiche Regeln)
  und E3.4 (Ruhe); Entwurf in Reihe 4 der Zeichenfläche.
