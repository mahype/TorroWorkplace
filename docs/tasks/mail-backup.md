# Aufgaben: Mail-Backup und Restore (#267)

Konzept: [mail-backup.md](../mail-backup.md). Jede Aufgabe einzeln nach `torro-task` ausführen.
Regeln: [CLAUDE.md](../../CLAUDE.md), `torro-api` bei Requests/Katalog/MCP und `torro-design`
bei Oberfläche. Die aktuelle Arbeit an #263 kann Cache-Dateien berühren; vor MB.3 deren
Stand prüfen und gegebenenfalls den Plan anpassen. Alle neuen Nutzertexte mit `tr()` und
deutscher Übersetzung. Kein echter Nutzeraccount für automatische Tests.

```text
MB.1 Format und Verträge ── MB.2 Archiv-Engine ── MB.3 Serverquelle ── MB.5 Restore
                                     └─────────── MB.4 Lokalquelle ───────┘
                                                  MB.5 ── MB.6 UI/CLI/MCP
```

### MB.1 Archivvertrag und API-Grundlage

- [x] erledigt
- **Modell:** Opus · **Braucht:** M2.3, M2.7 · **Dateien:** `Cargo.toml`, `Cargo.lock`,
  neu `crates/torro-mail-backup/{Cargo.toml,src/lib.rs,src/format.rs,tests/fixtures/**}`,
  `crates/torro-ipc/src/{lib,api,mail}.rs`, `crates/torroworkplace/Cargo.toml`,
  `crates/torroworkplace/src/{core,mcp,pim,audit,daemon}.rs`,
  `crates/torro-core/src/i18n/de.rs`,
  `docs/{mail-backup,api,api-reference}.md`
- Versioniertes Manifest, sichere Archivpfade und Quoten festlegen. Eine kleine anonyme
  Beispielstruktur dokumentieren und mit Schema-/Serde-Tests prüfen. Dateiname und
  interner Pfad dürfen nie aus ungeprüften IMAP-Namen übernommen werden. ZIP-Abhängigkeit
  nur über `[workspace.dependencies]`, ZIP64 aktiv; keine Inhalte vollständig im RAM halten.
- `Request` und Katalog für `mail.backup_create`, `mail.backup_status`,
  `mail.backup_verify`, `mail.restore_plan` und `mail.restore_apply` samt Antwort-/Fehler-
  typ, Abbruch, Kontenwahl, Quelle, Datum und archivierter Job-ID entwerfen. Zugriffsregeln
  und erlaubten Backup-Ordner mit App-Schlüssel versus Script-/Client-Schlüssel festlegen;
  auf beliebige Pfade und externe Dateiverfügbarkeit in `docs/api.md` eingehen. Der erste
  nutzbare Slice ist offline `verify`; übrige Befehle erst mit ihrer Engine freischalten.
- **Oberfläche:** noch keine. **MCP:** `mail_backup_verify` für erlaubte Archive;
  übrige Tools werden mit den jeweiligen nutzbaren Slices aktiviert. Echten Restore nur
  mit expliziter Freigabe für das Zielkonto. Vertrags- und Rechte-Tests, API-Referenz.

### MB.2 Archiv schreiben und offline prüfen

- [x] erledigt
- **Modell:** Opus · **Braucht:** MB.1 · **Dateien:**
  `crates/torro-mail-backup/src/{lib,format,writer,verify}.rs`,
  `crates/torro-mail-backup/tests/{archive,fixtures/**}.rs`,
  `crates/torroworkplace/Cargo.toml`, `crates/torroworkplace/src/mail/{backup,mod}.rs`,
  `docs/mail-backup.md`
- Rohbytes pro EML streamen, SHA-256 je Eintrag und `SHA256SUMS` berechnen, CSV/README/
  Manifest schreiben und Abschlussstatus erst nach Verifikation vergeben. Dateischreiben
  im erlaubten Ordner mit temporärem Namen und atomarer Veröffentlichung. `verify` ist
  offline und weist Zip-Slip, doppelte Pfade, unbekannte Version, Bomben, Inkonsistenzen,
  veränderte EML und nicht vollständige Archive mit konkreten Gründen aus.
- **Abnahme:** ZIP mit Systemwerkzeug und EML ohne Torro lesen; fehlerhafte Fixtures
  abweisen; sehr große Nachricht ohne proportionalen RAM-Verbrauch.

### MB.3 Serverquelle vollständig lesen

- [x] erledigt
- **Modell:** Opus · **Braucht:** MB.2 · **Dateien:**
  `crates/torro-mail-imap/src/{imap,lib}.rs`, `crates/torro-mail-imap/tests/{imap,imap_support/**}.rs`,
  `crates/torroworkplace/src/mail/{backup,mod}.rs`, `crates/torroworkplace/src/{core,mcp}.rs`,
  `docs/mail-backup.md`. **Freigegebene technische Ergänzungen:**
  `crates/torro-core/src/i18n/de.rs` (neue Meldungen und doppelter Basiseintrag),
  `crates/torroworkplace/src/mail/sync.rs` (instrumentierter IMAP-Testserver),
  `crates/torro-ipc/src/api.rs` (Katalogtests nach #263),
  `crates/torroinbox/src/app.rs` (Rebase-Testwert),
  `crates/torroworkplace/src/{inbox,mail/read}.rs` (Baseline-Testregressionen).
- Read-only Ordnerinventar und UID-Suche, `BODY.PEEK[]` in begrenzten Blöcken,
  `INTERNALDATE`, alle übertragbaren Flags/Keywords. `\All`-Doppelzählung vermeiden,
  Datum inklusiv und nach tatsächlichem `INTERNALDATE` filtern. Schlussinventar mit
  `UIDVALIDITY`/UID-Abgleich und begrenzter Nachholung neuer Mails. Fehler, Wechsel und
  Instabilität nie als complete ausgeben. Fortschritt/Abbruch und Job-Status anbinden.
- **Abnahme:** instrumentierter IMAP-Server mit mehreren Ordnern, großen Literalen,
  Keywords, Unicode und Änderung während des Laufs; Quelleninventar gleich Archiv-
  inventar oder klarer unvollständiger Bericht.

### MB.4 Lokale Originale und Lokalquelle

- [ ] erledigt
- **Modell:** Opus · **Braucht:** MB.2 und Abstimmung mit #263 · **Dateien:**
  `crates/torroworkplace/src/mail/{store,backup,mod}.rs`,
  `crates/torroworkplace/src/mail/store_corpus.rs`,
  `crates/torroworkplace/src/mail/tests.rs`, `docs/{mail-backup,mail}.md`
- Originaldepot für rohe EMLs mit Hash, Ordnerbezug, Größen-/Aufbewahrungsregel und
  migrationsverträglichem Index. Lokal-Backup liest ausschließlich dieses Depot;
  bestehende Header-/Body-Zeilen ohne Original erscheinen als fehlend. Keine heimliche
  Netz-Nachholung. Cache-Löschung und #263 dürfen nicht versehentlich das einzige
  Backup-Original entfernen; Einwilligung/Platzbedarf in der UI später sichtbar machen.
- **Abnahme:** offline vollständig gesicherter lokaler Bestand, Cache-Lücke gemeldet,
  Datum und Ordner korrekt, keine IMAP-Verbindung im Lokal-Modus.

### MB.5 Testlauf und echter Restore

- [ ] erledigt
- **Modell:** Opus · **Braucht:** MB.3, MB.4 · **Dateien:**
  `crates/torro-mail-backup/src/{verify,restore}.rs`,
  `crates/torro-mail-imap/src/imap.rs`, `crates/torro-mail-imap/tests/{imap,imap_support/**}.rs`,
  `crates/torroworkplace/src/mail/{backup,mod}.rs`,
  `crates/torroworkplace/src/{core,mcp}.rs`, `docs/{mail-backup,api}.md`
- Planen mit offline Integritätsprüfung und ausschließlich `EXAMINE`/read-only
  Zielabfragen; auch lokal kein Journal im Testlauf. Ordner-Mapping und Unklarheiten
  ausgeben. Echt: vor jeder Schreibphase erneut prüfen, fehlende Ordner erstellen,
  EML per APPEND mit Datum/Flags/Keywords, danach Inhalt/Anzahl prüfen. Journal und
  Abgleich nach unbestätigtem APPEND verhindern stille Duplikate. Unvollständige Archive
  nur nach separater Bestätigung; vorhandene Nachrichten nie löschen.
- **Abnahme:** Testserver protokolliert null Schreibbefehle im Testlauf;
  leerer/teilweise gefüllter Zielserver, Verbindungsbruch direkt nach APPEND,
  Wiederholung, unklare Kollision und fremde Keywords geprüft.

### MB.6 Mail-Oberfläche, CLI und sichere MCP-Wege

- [ ] erledigt
- **Modell:** Opus · **Braucht:** MB.5 · **Dateien:**
  `crates/torro-settings-ui/src/mail.rs`,
  `crates/torro-settings-ui/src/mail/{accounts,backup}.rs`,
  `crates/torroinbox/src/{app,hub}.rs`,
  `crates/torroworkplace/src/{cli,mcp}.rs`,
  `crates/torro-core/src/i18n/de.rs`, `docs/{settings,api-reference,mail-backup}.md`
- Unter Mail-Kontoeinstellungen den Ablauf aus dem Mock-up anbinden: Quelle,
  Datum, Speicherort, Fortschritt/Abbruch, Verify, Restore-Plan und echte Bestätigung,
  verständliche Fehler und Hinweis auf unverschlüsseltes ZIP. CLI als
  `torroworkplace ctl mail ...` mit derselben Engine, `--json`, Exit-Codes und
  nicht-interaktivem Ablauf. MCP nur für berechtigte Befehle auf erlaubten Archiven,
  keine willkürlichen Dateipfade. UI-Texte `tr()` und Deutsch.
- **Abnahme:** reale UI-Zustände in Mini/Schmal/Mittel/Breit und hell/dunkel sichten;
  CLI-Argumente und JSON, Rechtegrenzen, parallele Jobs und Abbruch testen;
  `torro-api-review` und `torro-design-review` ohne blockierende Befunde.

Vor Abschluss von #267: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `TORRO_LANG=de cargo test --workspace`, `scripts/ci-local.sh` und
manuelle Prüfung eines anonymen vollständigen Testarchivs.
