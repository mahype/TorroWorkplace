# Aufgaben: Eingang (Inbox), Schritte E1–E3

Konzept: [eingang.md](../eingang.md). Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill
`torro-task`. Wer einen Request anfasst, lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`.

E1–E3 bringen den Eingang **ohne Mail und ohne fremde Anmeldung**: Kern, Meldungen der Suite,
Agenten (Claude Code, Codex, Gemini CLI) und die App `torroinbox`. Mail kommt mit E4 (Konzept
[mail.md](../mail.md)), KI mit [ki.md](../ki.md).

> **Erst anfangen, wenn M7.1 und M7.2 erledigt sind** ([tasks/mail.md](mail.md)). Dort werden gerade
> die CalDAV- und Mail-Konten in Kern und Steuerzentrale gebaut, in denselben Dateien
> (`torro-ipc/src/{lib,api}.rs`, `torroworkplace/src/{core,mcp,clients,pim}.rs`, `docs/api.md`,
> `docs/api-reference.md`, Wurzel-`Cargo.toml`). Das gilt auch für E1.1, obwohl es meist neue Dateien
> anlegt. Danach von einem frischen Stand des Arbeitszweigs aus beginnen.
>
> **Auch A.3 (Freigaben) vorher abschließen.** A.3 ändert dieselben Stellen (`torro-ipc`, `core.rs`,
> `mcp.rs`, Test `names_follow_the_rules`) und wird für E2.1 ohnehin gebraucht. Aufgaben, die den
> Katalog ändern (E1.1, E2.4, E2.5, E3.5), laufen nie parallel zu anderen Katalog-Aufgaben (A.5, A.11,
> M-, G-, P-Aufgaben).

```
E1.1a ─┬─ E3.1 ── E3.2                  ← sofort möglich, nur neue Dateien
       │
M7.1 + M7.2 (mail.md), A.3 (api-mcp.md)
       │
       └─ E1.1 ── E1.2 ─┬─ E1.3 ── E2.2 ─┬─ E2.3 (braucht A.7)
                        │                └─ E2.4
                        ├─ E2.1
                        ├─ E3.3 (nach A.8), E3.6, E3.7
                        └─ E3.4 (braucht E2.1)
E3.5 braucht E3.2, E1.2, P.2 · E2.5 braucht E2.3, E3.2, A.8 und die Freigabe ihres Sicherheitsteils
```

**Ausnahme von der Wartebedingung:** E1.1a, E3.1 und E3.2 legen nur neue Crates an (plus je eine
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
   `window` nur bei Hook-Einträgen.
7. **Protokoll ohne Inhalte:** `title`, `body`, `thread_key` landen nie in `audit.jsonl`.

## Offene Punkte (vor der jeweiligen Aufgabe klären)

1. **Schnittstelle `Rights`** (E1.1): neues Feld `inbox_sources: Selection` (welche Quellen ein Agent
   sieht). Neues optionales Feld, also verträglich, aber `Rights` ist festgelegt: **Freigabe nötig.**
2. **Schnittstelle `Event`** (E3.4): Für „Ruhe bei belegten Terminen“ braucht das Modell Frei/Belegt
   (`TRANSP`). Fehlt heute. **Freigabe nötig**, sonst startet E3.4 ohne Ruhe und sie kommt nach.
3. **Sicherheitsteil von E2.5** (Rückfragen beantworten) vor Beginn freigeben; dazu gehört eine
   Erweiterung von Punkt 3 in „Was nie nach außen geht“ ([api.md](../api.md)) auf Antworten auf
   Rückfragen.
4. **A.6 (Agents & API)** braucht auf der Detailseite zusätzlich die Auswahl der Inbox-Quellen. Wird
   in A.6 mit erledigt, sobald E1.1 steht.

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
- [ ] erledigt
- **Modell:** Opus · **Braucht:** E1.1a, M7.1, M7.2, A.3, offener Punkt 1 · **Dateien:**
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
- [ ] erledigt
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
- Im Dienst: Vertagtes zur Zeit aufwecken, einmal täglich aufräumen.
- Die App fragt wie Kalender und Aufgaben im Takt nach (`iced::time::every`); ein `subscribe` gibt es
  noch nicht und gehört nicht in diese Aufgabe.
- **Abnahme:** Tests im Temp-Verzeichnis: Anlegen, Zusammenführen, Erledigen, Vertagen und Aufwachen
  (feste Uhr), Neustart verliert nichts, Client ohne Quelle → `not-found`, Hook-Client liest →
  `not-found`, Client löst fremden Vorgang auf → keine Wirkung, Drossel → `busy`, Datei 0600; ist A.5
  schon da: `title`, `body`, `thread_key` stehen nicht in `audit.jsonl`.

### E1.3 MCP, `torroworkplace notify` und Client für die App
- [ ] erledigt
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
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** E1.2, A.3 · **Dateien:** `crates/torroworkplace/src/{inbox,approvals,pim}.rs`
- Jede offene Freigabe ist ein Eintrag (`approval`, dringend, Aktionen „Allow“/„Deny“, die die Oberfläche
  als `approvals.decide_approval` schickt; Name aus A.3). Entschieden oder abgelaufen → erledigt sich
  selbst. Die Zusammenfassung enthält Agententext und ist `untrusted`.
- Kontoprobleme (Passwort abgelehnt, neue Anmeldung nötig, Sync scheitert länger als 30 Minuten) sind
  Einträge `account_problem` mit Link in die Steuerzentrale; behoben → erledigt sich selbst.
- **API/MCP:** keine neuen Befehle; `approvals.decide_approval` bleibt **Never**.
- **Abnahme:** Tests: Freigabe anlegen → Eintrag, entscheiden → erledigt, Ablauf → erledigt;
  Kontoproblem kommt und geht; ein Agent kann beide nicht erledigen (`forbidden`).

### E2.2 `torroworkplace hook`: Agenten melden sich
- [ ] erledigt
- **Modell:** Opus · **Braucht:** E1.3 · **Dateien:** `crates/torroworkplace/src/hook.rs` (neu),
  `crates/torroworkplace/src/{cli,main}.rs`, `crates/torroworkplace/tests/fixtures/hooks/*.json` (neu)
- `torroworkplace hook claude-code|codex|gemini` liest das JSON des Hooks von stdin und schickt
  `inbox.post_item` bzw. `inbox.resolve_items` mit einem Hook-Schlüssel (`TORRO_TOKEN`):

  | Agent | Ereignis | Wird zu |
  |---|---|---|
  | Claude Code | `Notification` mit `permission_prompt`, `agent_needs_input`, `elicitation_dialog` | `agent_waiting` |
  | Claude Code | `Stop` | `agent_done` |
  | Claude Code | `UserPromptSubmit`, `PostToolUse` | erledigt das Warten dieser Sitzung |
  | Claude Code | `SessionEnd` | erledigt alles dieser Sitzung |
  | Codex | `PermissionRequest` (nur beobachten, keine Entscheidung) / `Stop` / `UserPromptSubmit`, `PostToolUse` | wartet / fertig / erledigt |
  | Gemini CLI | `Notification` (`ToolPermission`) / `AfterAgent` | wartet / fertig |

  `idle_prompt` wird ignoriert (`Stop` deckt es ab). `thread_key` = `<agent>:<session_id>` (der Kern
  stellt den Client davor). Projekt = letzter Teil von `cwd`; Branch aus `.git/HEAD` lesen (keinen
  Prozess starten). Die Meldung des Agenten geht als `body` mit (`untrusted`).
- **Darf den Agenten nie aufhalten oder ändern:** schreibt **nichts** auf stdout (Claude Code wertet
  stdout von Hooks aus), endet immer mit Exit-Code 0, höchstens 500 ms für Verbindung und Antwort;
  läuft der Dienst nicht, passiert nichts (kein Start des Dienstes).
- Unter Linux die Kette der Elternprozesse (`/proc/<pid>/stat`, höchstens 32 Stufen) als `agent.pids`
  mitschicken, für E2.4. Auf macOS und Windows leer (`#[cfg(target_os = "linux")]`).
- Bis E2.3 steht, legt man den Hook-Client von Hand an (`torroworkplace ctl`, App-Schlüssel).
- **API/MCP:** nutzt E1.1.
- **Abnahme:** Fixture-Tests je Ereignis → richtiger Request; unbekanntes Ereignis → nichts; kein
  Byte auf stdout; ohne Dienst Exit-Code 0 in unter 500 ms.

### E2.3 Hooks beim Verbinden eines Agenten einrichten
- [ ] erledigt
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

### E2.4 Ins Fenster des Agenten springen
- [ ] erledigt
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
- **Abnahme:** Tests mit `torro-wm`-Fake (Fenster mit pid): Treffer, Fenster weg → `unsupported`;
  `file:`-Link → `unsupported`; `window` an einem Nicht-Hook-Eintrag wird ignoriert.

### E2.5 Rückfragen von Agenten im Eingang beantworten
- [ ] erledigt
- **Modell:** Opus · **Braucht:** E2.3, E3.2, A.8, offener Punkt 3 · **Dateien:** werden mit der
  Freigabe festgelegt (voraussichtlich `hook.rs`, `approvals.rs`, `inbox.rs`, `torro-ipc`, `torroinbox`,
  `docs/api.md`)
- Claude Code und Codex fragen über den Hook `PermissionRequest` „darf ich?“. Der Hook legt eine
  Rückfrage an und wartet bis zu 30 s auf die Entscheidung des Menschen im Eingang oder in der
  Benachrichtigung („Allow“ / „Deny“). Ohne Entscheidung gibt der Hook **keine** Antwort, und der
  Agent fragt wie gewohnt im Terminal. Nie automatisch erlauben.
- **API:**
  - `inbox.ask_question { question, options }` · Write – ein Agent oder Hook stellt dem Menschen eine
    Frage. MCP `inbox_ask_question`.
  - `inbox.question_status { id }` · Read – Antwort abholen. MCP `inbox_question_status`.
  - `inbox.answer_question { id, option }` · Admin · **Never**, nur App-Schlüssel (Erweiterung von
    Punkt 3 in api.md; in `names_follow_the_rules` unter `kept_back`).
- **Sicherheitsteil zur Freigabe:** genauer Wortlaut des Befehls ungekürzt sichtbar, Entscheidung an
  genau diese Rückfrage gebunden, fremder Text als `untrusted` gekennzeichnet, Antwort nur über den
  App-Schlüssel.
- **Abnahme:** Tests: Zeitablauf → keine Ausgabe; „Deny“ → Ablehnung im Format des Agenten;
  Client-Schlüssel kann nicht antworten.

## E3 App `torroinbox`

### E3.1 App-Gerüst
- [ ] erledigt
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
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** E3.1 · **Dateien:** `crates/torroinbox/src/{app,keys}.rs`
- `j`/`k` bewegen, `Enter` öffnen, `e` erledigt, `u` rückgängig (Hinweis 5 s), `s` vertagen (Later today,
  Tomorrow, Next week, Pick…), `m` Quelle stumm (mit Nachfrage), `l` Spur wechseln (fragt „Always for
  this source?“). Alles auch mit der Maus.
- `Enter` öffnet bis E2.4 nur `http`/`https`-Links im Browser; mit E2.4 geht es über `inbox.open_item`.
- Läuft mit der Demo-Quelle, bevor der Kern steht.
- **API/MCP:** nutzt `inbox.set_item_done`, `snooze_item`, `set_item_lane`, `mute_source`.
- **Abnahme:** Tests je Taste, Rückgängig.

### E3.3 Mini, Leiste und Tray
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** E3.1, A.8 (gleiche Tray-Datei) · **Dateien:** `crates/torroinbox/src/glance.rs` (neu),
  `crates/torroinbox/src/app.rs`, `crates/torroworkplace/src/tray/linux.rs`
- Mini/Leiste: „3 for you“ und die drei obersten Einträge; Dringendes hervorgehoben (Design-Regel:
  nicht in Torro-Rot).
- Tray: **eine** Zahl für offene Einträge „für mich“ (Freigaben sind jetzt Einträge, die Zahl aus A.8
  geht darin auf), Untermenü mit den fünf neuesten, „Open Inbox“.
- **Abnahme:** Sichtprüfung Mini/Leiste; Test der Tray-Zahl.

### E3.4 Desktop-Benachrichtigungen und Ruhe
- [ ] erledigt
- **Modell:** Opus · **Braucht:** E1.2, E2.1, offener Punkt 2 · **Dateien:** `crates/torroworkplace/src/{notify,inbox}.rs`,
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
- [ ] erledigt
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
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** E3.1 · **Dateien:** `crates/torroworkplace/src/apps.rs`,
  `crates/torroinbox/src/main.rs`, `docs/app-catalog.md`
- Startbar aus Tray und Steuerzentrale (Apps), Fensterklasse `torro-inbox`, Argumente `--lane` und
  `--source` für Fenster je Ausschnitt, gemerkt in Workspace-Vorlagen. Im App-Katalog „Inbox“ eintragen;
  „Benachrichtigungen“ und „Freigaben-Eingang“ als darin aufgegangen markieren.
- **Abnahme:** App startet aus dem Tray; Vorlage mit `--lane info` stellt das Fenster wieder her.

### E3.7 Steuerzentrale: Seite „Inbox“
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** E1.2, E3.4 · **Dateien:** `crates/torroworkplace/src/ui/inbox.rs` (neu),
  `crates/torroworkplace/src/ui/mod.rs`
- **Oberfläche:** Gruppe „Notifications“ mit „Quiet during busy events“; Gruppe „Rules“ mit gemerkten
  Spur-Regeln und stummen Quellen, je mit „Remove“.
- **API/MCP:** nutzt `inbox.list_rules`, `inbox.remove_rule`, `settings.*`.
- **Abnahme:** Sichtprüfung (Breit, Schmal, hell/dunkel); Tests der Nachrichten.

## Danach (Überblick, Aufgaben folgen)

- **E4 Mail als Quelle**, nach dem Mail-Kern (M2/M3 in [mail.md](../mail.md)); dort entscheiden: Spur
  „Belege“, „erledigt“ archiviert oder nicht.
- **E5 Nextcloud, GitHub, ntfy** als Quellen.
- **E6 Ruhe und Ordnung:** Screener, Lernen, Zusammenführen über Quellen; mit [ki.md](../ki.md) Spur
  vorschlagen und Zusammenfassungen.
- **E7 Slack**, danach Messenger (zweiter Schritt).
