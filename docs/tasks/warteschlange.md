# Warteschlange des Issue-Dispatchers

Stand: 2026-11-25, freigegeben von Sven. Warteschlange für den T3-Issue-Dispatcher (geplante Aufgabe
„TorroWorkplace – Issue-Dispatcher“). Regeln für die Arbeit: [CLAUDE.md](../../CLAUDE.md), Detailfragen:
[entscheidungsregeln.md](../entscheidungsregeln.md), Issue-Workflow: [issue-workflow.md](../issue-workflow.md).

Wie alles unter `docs/tasks/` ist diese Datei eine Bestandsaufnahme: Für jede Aufgabe gilt das Issue mit seinem Plan.
Die Datei legt nur fest, **wann** welches Issue gestartet und gemergt wird.

## 1. Zweck und Regeln

- **Wer baut:** nur Claude in T3, Opus für mittlere und große Aufgaben, Sonnet bzw. Fable für kleine. Das Code-Review
  macht Codex in einem eigenen Thread. Bei Oberflächen-Arbeit legt der Designer vorher die Gestaltung fest und prüft
  nachher; der PR enthält Vorher-nachher-Screenshots.
- **Bahnen:** Drei Bahnen laufen parallel, innerhalb einer Bahn nacheinander. Die nächste Aufgabe einer Bahn startet,
  wenn die Bahn frei ist (kein aktiver Coding-Thread, kein offener ungemergter PR der Bahn) und die vorherige Aufgabe
  gemergt ist. Höchstens 3 Coding-Threads gleichzeitig.
- **Prüfung auf Überholung vor jedem Start:** Ist das Issue noch offen? Gibt es gemergte PRs mit „Closes“, „Related
  to“ oder „Refs“ auf das Issue? Neue Kommentare? Ist es schon auf `main`? Wenn überholt: nicht starten, melden.
- **Namen:** Coding-Threads `[Bahn N] #NNN Kurztitel`, Review-Threads `[Review] #NNN Codex`. PR-Titel mit Präfix
  `[auto] `; jeder PR endet mit `## TL;DR` (Issue, Problem, Änderung, Besonderheiten).
- **Merge:** Kein Merge durch Agenten. Gemergt wird nur von Sven, in der Merge-Reihenfolge (Abschnitt 3). Vor jedem
  Merge holt der PR `main` nach und erzeugt die API-Referenz mit `TORRO_BLESS` neu.
- **Danach:** Ist die Warteschlange leer, darf der Dispatcher kleine, risikoarme Issues nach denselben Regeln nehmen.

## 2. Bahnen

Status: `offen`, `läuft (Thread …)`, `PR #NNN`, `gemergt (PR #NNN)`, `überholt`.

### Bahn 1 – Build, Einstellungen, Doku

| Reihenfolge | Issue | Modell | UI | Umfang | Abhängigkeit | Status |
|---|---|---|---|---|---|---|
| 1 | [#808 Assistent: Qwen3-TTS als eigenes Programm mit eigenem Workspace auslagern](https://github.com/mahype/TorroWorkplace/issues/808) | Opus | nein | mittel | keine; ändert `Cargo.lock`, daher zuerst mergen; CUDA-Lauf auf dem Mac nicht prüfbar | läuft (Thread [Bahn 1] #808) |
| 2 | [#687 Steuerzentrale: Reiter auf der Seite eines MCP-Clients](https://github.com/mahype/TorroWorkplace/issues/687) | Sonnet | ja | klein bis mittel | #808 gemergt | offen |
| 3 | [#335 KI: Strata gegen ein echtes Programm und in der Oberfläche prüfen](https://github.com/mahype/TorroWorkplace/issues/335), nur der Doku-Punkt in `docs/settings.md` (Teil-PR „Related to #335“) | Sonnet | nein | klein | #687 gemergt | offen |

### Bahn 2 – Kern, Mail

| Reihenfolge | Issue | Modell | UI | Umfang | Abhängigkeit | Status |
|---|---|---|---|---|---|---|
| 1 | [#854 Kontakte: Pin von Hand setzen (GEO schreiben)](https://github.com/mahype/TorroWorkplace/issues/854) | Opus | ja | mittel | keine (#357 erledigt) | läuft (Thread [Bahn 2] #854) |
| 2 | #946a: [#946 Mitteilungszentrale: GitHub-Benachrichtigungen nur aus ausgewählten Repos](https://github.com/mahype/TorroWorkplace/issues/946), nur Kern, API und MCP (`inbox.set_source_repos` als Admin, Filter in `sources/github.rs`), Teil-PR „Related to #946“; die Oberfläche wird ein neues Issue für den Designer | Opus | nein | mittel | #854 gemergt | offen |
| 3 | #452a: [#452 Mail: farbige Fahnen wie in Apple Mail und Priorität des Absenders](https://github.com/mahype/TorroWorkplace/issues/452), nur Kern (Apple-Schema `$MailFlagBit0-2`, Priorität aus Kopfzeilen, API und MCP), Teil-PR „Related to #452“; Farbmenü und Darstellung werden ein neues Issue für den Designer; Thunderbird-Labels vorerst nicht | Opus | nein | mittel | #946a gemergt | offen |

### Bahn 3 – Assistent, Inhaltskarten

Streng nacheinander: Alle Aufgaben ändern dieselben Dateien (`content_card`, `CardRef`, Assistent-Chat).

| Reihenfolge | Issue | Modell | UI | Umfang | Abhängigkeit | Status |
|---|---|---|---|---|---|---|
| 1 | [#962 Assistent: Aufträge im Chat starten, sehen und steuern](https://github.com/mahype/TorroWorkplace/issues/962) | Opus | ja | mittel bis groß | keine (#961 erledigt) | läuft (Thread [Bahn 3] #962) |
| 2 | [#791 Inhaltskarten: Vorschau-Anbieter je Dateiart](https://github.com/mahype/TorroWorkplace/issues/791) | Opus | ja | mittel bis groß | #962 gemergt | offen |
| 3 | [#795 Inhaltskarten: Bild eines MCP-Servers](https://github.com/mahype/TorroWorkplace/issues/795) | Opus | ja | mittel | #791 gemergt (zwingend) | offen |
| 4 | [#792 Inhaltskarten: Videos](https://github.com/mahype/TorroWorkplace/issues/792) | Sonnet | ja | klein bis mittel | #795 gemergt | offen |
| 5 | [#793 Inhaltskarten: Landkarten](https://github.com/mahype/TorroWorkplace/issues/793) | Sonnet | ja | klein bis mittel | #792 gemergt | offen |
| 6 | [#794 Inhaltskarten: PDF](https://github.com/mahype/TorroWorkplace/issues/794) | Sonnet | ja | klein bis mittel | #793 gemergt | offen |

## 3. Merge-Reihenfolge

#808, #687, #854, #335 (Doku), #962, #946a, #452a, #791, #795, #792, #793, #794.

## 4. Festgelegte Entscheidungen

| Issue | Entscheidung |
|---|---|
| #791 | MCP-Vorschau `Never` (von Sven am 2026-11-25 bestätigt; die vorsichtigere Variante nach Regel 2 und 7). Settings › Extensions mit dem Designer; Mockups in `docs/prototypes/inhaltskarten/`. |
| #792 | YouTube wie bisher; Vimeo, PeerTube und direkte Videodateien als Karte, die im Browser öffnet; kein eigener Player (Regel 2). |
| #793 | Vorher im Code prüfen, wo der Schalter `maps` heute liegt, und ihn dort nutzen, nicht verschieben (Regel 2). |
| #794 | Ohne neue Abhängigkeit: Ohne Renderer bleibt das Symbol (Regel 3); Titel, Seitenzahl und Größe über das vorhandene PDF-Lesen aus #893 (PR #907). Braucht es eine neue Abhängigkeit: anhalten und Sven fragen. |
| #962 | Der Designer legt die Gestaltung vor der Umsetzung fest (Regel 5). |
| Teil-PRs | #946a, #452a und der Doku-Punkt aus #335 mit „Related to“ statt „Closes“; abgespaltene Oberflächen werden eigene Issues für den Designer (Regel 5). |

## 5. Gemeinsame Konfliktstellen

`crates/torro-ipc/src/api.rs` (Befehlskatalog), `crates/torroworkplace/src/mcp.rs` (MCP-Tools),
`crates/torro-core/src/i18n/de.rs`, `docs/api-reference.md` (generiert) und
`Cargo.lock`. Deshalb wird nacheinander gemergt, und jeder Folge-PR holt `main` nach (Regel 1).

## 6. Pflege

Der Dispatcher bzw. der Projekt-Bot aktualisiert die Spalte „Status“ über kleine Doku-PRs. Erledigte Zeilen bleiben
mit dem Link auf ihren PR stehen.
