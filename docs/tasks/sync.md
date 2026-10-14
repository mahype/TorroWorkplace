# Aufgaben: Abgleich über Nextcloud (N.1–N.8, Stufe 1)

Konzept: [sync.md](../sync.md). Regeln für die Arbeit: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill
`torro-task`, Nextcloud: Skill `nextcloud` bzw. Agent `nextcloud-expert`, Oberfläche: Skill `torro-design`. Wer einen
Request anfasst, lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`.

> **Stand 2026-10-14: Entwurf.** Vor dem Start freigeben: Punkte 1–6 unter „Offen“ im Konzept (neuer Bereich `sync`, Feld `device` an `Entry`,
> neuer Punkt 9 in „Was nie nach außen geht“, Verschlüsselung, fehlender Antrieb, Mail-Inhalte, Ordner, Zoom).
> Stufe 1 gleicht den ganzen Assistenten ab (Gedächtnis, Profil, Persönlichkeit, Bots, Unterhaltungen) und die
> Einstellungen aus der Liste im Konzept.

```
N.2 (WebDAV) ───────────────────────────────────────┐
N.1 ──┬── N.3 (Protokoll, ohne Netz) ── N.4 (lokale Speicher) ──┴── N.5 (Abgleich) ── N.6 (Rechner bestätigen)
      │                                  └── N.8 (Handänderungen)
      └── N.7 (Steuerzentrale, gegen die Gerüst-Antworten; fertig nach N.6)
```

N.2 und N.3 laufen parallel (verschiedene Crates). N.7 kann nach N.1 beginnen.

### N.1 Gerüst: Bereich `sync`, Requests, Katalog, `sync.json`
- [ ] erledigt
- **Modell:** Opus · **Braucht:** Freigabe des offenen Punkts 1 · **Dateien:**
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torro-ipc/src/sync.rs` (neu), `crates/torroworkplace/src/sync/mod.rs`
  (neu), `crates/torroworkplace/src/{core,mcp,approvals}.rs`, `docs/api.md` (Punkt 9, Bereich in der Tabelle),
  `docs/api-reference.md`
- `Area::Sync`; Requests und Katalog nach der Tabelle „Drei Wege“ im Konzept: `sync.get_status` (Read),
  `sync.update_sync { enabled?, account?, areas? }` (Admin), `sync.run_now` (Write), `sync.list_devices` (Read),
  `sync.rename_device` (Write), `sync.confirm_device` (Admin, `Mcp::Never`, in `kept_back`),
  `sync.remove_device` (Admin; mit der eigenen Kennung = „Remove this computer from sync“, bei einem unbestätigten
  Rechner = ablehnen), `sync.list_conflicts` (Read), `sync.resolve_conflict { id, keep }` (Admin; „Restore?“ ist
  `keep: other`).
- Rechte: `sync` nicht in `Area::in_templates`; in `loops_back` für `ASSISTANT_CLIENT` und `BOT_CLIENT` alles außer
  den Lesebefehlen. `sync.list_conflicts` gibt die Fassungen nur mit Leserecht im Bereich des Schlüssels
  (`assistant`, `bots`, `settings`), sonst nur Kennung, Art und Rechner. `sync.resolve_conflict` verlangt zusätzlich
  das Recht des passenden Fachbefehls (`assistant.update_profile`, `assistant.forget`, `bots.save_bot`,
  `settings.set_setting`).
- Typen in `torro-ipc/src/sync.rs`: `SyncStatus`, `SyncArea` (`assistant`, `settings`; Stufe 2 erweitert),
  `Device` (Kennung, Name, System, zuletzt gesehen, bestätigt, dieser Rechner), `Conflict` (Kennung, Art, Schlüssel
  ohne Inhalte im Protokoll, beide Fassungen mit Rechner und Zeit), `Keep` (`current` | `other`).
- `<config>/sync.json` (0600, atomar): an/aus, Konto-Kennung (nur `AccountKind::Nextcloud`), Bereiche,
  Geräte-Kennung (16 Hex, beim ersten Lesen erzeugt), Name, bestätigte Rechner mit öffentlichem Schlüssel.
- Handler: `get_status`, `update_sync` (speichert nur; `account` muss ein verbundenes Nextcloud-Konto sein, sonst
  `not-found`), `rename_device`, `list_devices` (nur dieser Rechner) echt; `run_now` → `not-connected` solange aus;
  der Rest liefert leere Listen bzw. `not-found`, bis N.5/N.6 sie füllen. Freigabe-Texte für `update_sync` (nennt
  Konto und Bereiche aus dem Kern), `remove_device`, `resolve_conflict` in `approvals.rs`, mit Nachschlage-Regel in
  `approvals::Facts` über die Kennung (Rechnername gekürzt, ohne Steuerzeichen). Protokoll nur mit Kennungen.
- Server-Anweisungen in `mcp.rs` um den Bereich `sync` ergänzen („only the person confirms a computer“).
- **Oberfläche:** N.7 · **API:** wie oben · **MCP:** je Eintrag ein Tool `sync_*`, außer `sync.confirm_device`
  (Never, Punkt 9).
- **Abnahme:** Handler-Tests mit Temp-Verzeichnis; Rechte-Tabelle deckt den Bereich; Test, dass
  `sync.confirm_device` auch mit Admin-Script-Schlüssel `forbidden` ist; Test, dass Assistent und Bot
  `sync.resolve_conflict` nicht dürfen; Test, dass `list_conflicts` ohne Leserecht in `assistant` keine Texte
  zeigt; Vertragstest Tool ↔ Katalog;
  `TORRO_BLESS=1 cargo test -p torro-ipc`.

### N.2 WebDAV auf die Dateien eines Nextcloud-Kontos
- [ ] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-nextcloud/src/{lib,files}.rs`,
  `crates/torro-nextcloud/tests/fixtures/**`, `crates/torro-nextcloud/Cargo.toml`
- `Nextcloud::files()` auf `remote.php/dav/files/<login>/`: `list(path, depth)` per `PROPFIND` mit `getetag`,
  `getcontentlength`, `resourcetype`, `oc:share-types`, `nc:mount-type`; `get`, `put` (mit `If-None-Match: *` bzw.
  `If-Match`), `mkcol` (auch Pfade mit Zwischenordnern), `move` (mit `Overwrite: F`), `delete`.
- XML mit `roxmltree` (gibt es). Pfadteile werden kodiert und geprüft (kein `..`, keine Steuerzeichen). Gleiche
  Regeln wie OCS: App-Passwort nur im `Authorization`-Header, nur an den eigenen Ursprung, keine Umleitung woandershin,
  `Debug` ohne Passwort. Fehler auf die vorhandenen Codes (`offline`, `auth`, `conflict` bei 412,
  `not-found`, `busy` bei 429/503 mit `Retry-After`).
- **Oberfläche/API/MCP:** keine (Bibliothek für N.5).
- **Abnahme:** `httpmock`-Tests für jede Methode, 207 mit mehreren `propstat`, 412, 423 (gesperrt), Umleitung auf
  fremden Ursprung, geteilter Ordner (`share-types` gesetzt), Umlaute und Leerzeichen im Pfad
  (`Torro Workplace/Sync`).

### N.3 Protokoll und Zusammenführen (ohne Netz)
- [ ] erledigt
- **Modell:** Opus · **Braucht:** N.1 · **Dateien:** `crates/torroworkplace/src/sync/{clock,op,merge}.rs` (neu),
  `crates/torroworkplace/src/sync/mod.rs`, `crates/torroworkplace/tests/fixtures/sync/**`
- `clock`: hybride logische Uhr (`<ms>-<zähler>-<gerät>`, nach Text sortierbar); `observe` zieht beim Lesen fremder
  Einträge nach.
- `op`: Eintrag `{ at, key, base, value | deleted }`, Schlüssel nach der Tabelle im Konzept als Enum (kein freier
  Text), Lesen und Schreiben eines Abschnitts (JSONL). Prüfung je Schlüssel mit den Grenzen der Requests
  (`MAX_NOTE_CHARS`, `profile_refusal`, `is_id`, Steuerzeichen; Einstellungen wie `settings.set_setting`);
  ungültige Zeilen werden übersprungen und gezählt.
- `merge`: Stand je Schlüssel mit Köpfen; neuester Kopf gilt; Konflikte für Texte (Gedächtnis, Profil,
  Persönlichkeit, Bot-Texte), nicht für Einstellungen und Metadaten der Unterhaltungen; Löschen gewinnt gegen eine
  gleichzeitige Änderung (Konflikt „Restore?“); Einträge der Unterhaltungen nur wachsend, nach Grabstein verworfen.
  Ergebnis unabhängig von der Lese-Reihenfolge, zweimal Lesen ändert nichts.
- Verdichten: aus den eigenen Einträgen den Schnappschuss bilden (je Schlüssel Köpfe und Grabsteine, alle Einträge
  der Unterhaltungen).
- **Oberfläche/API/MCP:** keine.
- **Abnahme:** Tests für jede Zeile der Konflikttabelle im Konzept; Eigenschaftstest (`proptest` nur, wenn schon im
  Workspace, sonst feste Vertauschungen): drei Rechner, beliebige Reihenfolge → gleicher Stand; Uhr mit
  vorgehendem Rechner; Schnappschuss + Rest = alle Abschnitte.

### N.4 Lokale Speicher anbinden
- [ ] erledigt
- **Modell:** Opus · **Braucht:** N.3 · **Dateien:** `crates/torroworkplace/src/sync/local.rs` (neu),
  `crates/torroworkplace/src/assistant/{mod,profile,store}.rs`, `crates/torroworkplace/src/bots/{mod,store}.rs`,
  `crates/torroworkplace/src/settings_service.rs`, `crates/torroworkplace/src/core.rs`,
  `crates/torro-ipc/src/assistant.rs` (Feld `device` an `Entry`), `docs/api-reference.md`
- **Ausgang:** Jede erfolgreiche Änderung über die vorhandenen Handler (`assistant.remember`, `forget`,
  `update_profile`, `bots.save_bot`, `delete_bot`, neue Einträge, `rename_conversation`, `pin_conversation`,
  `delete_conversation`, Aufbewahrung, `settings.set_setting`/`update_settings` für die erlaubten Schlüssel) schreibt
  Einträge in `<state>/sync/outbox.jsonl`, nur wenn der Abgleich an ist und der Bereich gewählt. `base` = die
  aktuellen Köpfe. Notizen „Nur auf diesem Computer“ nie.
- **Eingang:** fremde Einträge über dieselben Speicherfunktionen eintragen (atomar wie bisher); Einträge der
  Unterhaltungen bekommen die nächste lokale `seq` und den Rechner als Herkunft; ein neuer Bot bekommt einen neuen
  Client der Art `bot` mit `Rights::none` und einen Eintrag im Eingang „Grant access?“. Einstellungen nur aus der
  Liste, `voice` und `startAtLogin` nie.
- **Erster Stand:** beim Einschalten den heutigen Stand dieses Rechners als Einträge (für N.5).
- Kennung „von welchem Rechner“ an `Entry` als optionales Feld (verträglich; Schnittstelle, darum vor dem Start
  freigeben lassen).
- **Oberfläche/API/MCP:** keine neuen; vorhandene Befehle verhalten sich gleich.
- **Abnahme:** Tests mit Temp-Verzeichnis: jede Änderung erzeugt den richtigen Eintrag; Eintragen fremder
  Einträge ergibt die richtigen Dateien (`memory.md`, `persona.md`, `bot.json`, `.jsonl`); `seq` steigt nur;
  Bot ohne Rechte; `voice` aus dem Abgleich wird verworfen, als einzelner Schlüssel wie als ganzes Objekt.

### N.5 Abgleich mit Nextcloud
- [ ] erledigt
- **Modell:** Opus · **Braucht:** N.2, N.4 · **Dateien:** `crates/torroworkplace/src/sync/{remote,engine}.rs` (neu),
  `crates/torroworkplace/src/sync/mod.rs`, `crates/torroworkplace/src/{core,daemon,inbox}.rs`
- Ordner anlegen (`Torro Workplace/Sync/`, `README.md`, `format.json`, `devices/<id>/device.json`), Prüfung „nicht
  geteilt“ vor jedem Takt.
- Hochladen: Ausgang 5 s nach der letzten Änderung als neuer Abschnitt (`.part` + `MOVE`); erst nach Erfolg aus dem
  Ausgang nehmen.
- Holen: beim Start, jede Minute, auf `sync.run_now`; `PROPFIND` Tiefe 0 auf `Sync/` (ETag), bei Änderung Tiefe 1
  auf die geänderten Rechner, neue Abschnitte `GET`. Gelesenes in `<state>/sync/seen.json`. Fehlt ein Abschnitt
  (verdichtet), den Schnappschuss lesen.
- Verdichten ab 50 Abschnitten oder 5 MB. `sync.remove_device` und Rechner mit `leaving`: Einträge mit ursprünglicher
  Uhr übernehmen, dann Ordner löschen.
- `sync.get_status`, `sync.list_devices`, `sync.list_conflicts`, `sync.resolve_conflict` mit echten Daten.
  Neue Konflikte als Eintrag im Eingang (Spur *For me*). Ohne Netz: `offline` im Stand, nichts geht verloren.
- Protokoll ohne Inhalte: Zahl der Einträge, Rechner, Dauer.
- **Oberfläche:** N.7 · **API/MCP:** aus N.1.
- **Abnahme:** Tests mit `httpmock` als Nextcloud und zwei Kernen in Temp-Verzeichnissen: Notiz auf A erscheint auf
  B; gleichzeitige Änderung der Persönlichkeit → ein Konflikt auf beiden, Lösen auf A löst ihn auch auf B; dieselbe
  Unterhaltung auf beiden fortgesetzt → alle Einträge auf beiden; offline ändern, später hochladen; Abbruch beim
  Hochladen hinterlässt keinen halben Abschnitt; geteilter Ordner hält an.

### N.6 Rechner bestätigen und Abschnitte signieren
- [ ] erledigt
- **Modell:** Opus · **Braucht:** N.5 · **Dateien:** `crates/torroworkplace/src/sync/{device,engine}.rs`,
  `crates/torroworkplace/src/{core,notify,inbox}.rs`, `Cargo.toml` (Wurzel: `ed25519-dalek` in
  `[workspace.dependencies]`), `crates/torroworkplace/Cargo.toml`
- Schlüsselpaar je Rechner (Ed25519): privat im Schlüsselbund (Dienst `torroworkplace`), öffentlich in `device.json`.
  Jeder Abschnitt und der Schnappschuss tragen eine Signatur (letzte Zeile `{"sig": …}` über alle Zeilen davor).
- Neuer Rechner im Ordner → Eintrag im Eingang und Benachrichtigung „New computer *Name* wants to sync. Allow?“ mit
  Kurzform des Schlüssels; `sync.confirm_device` nur mit App-Schlüssel. Einträge unbestätigter Rechner und Abschnitte
  mit falscher Signatur werden nicht eingetragen; geänderter Schlüssel eines bestätigten Rechners → anhalten und
  melden.
- Beim Einschalten auf einem weiteren Rechner zeigt die Steuerzentrale die vorhandenen Rechner zum Bestätigen.
- **Oberfläche:** N.7 · **API:** `sync.confirm_device` · Admin · **MCP:** Never (Punkt 9).
- **Abnahme:** Tests: fremder Abschnitt ohne Signatur wird nicht eingetragen; untergeschobener Abschnitt im Ordner
  eines bestätigten Rechners wird abgelehnt; erst nach `confirm_device` kommen die Einträge an; privater Schlüssel in
  keiner Antwort, keinem Protokoll, keinem `Debug`.

### N.7 Steuerzentrale › Sync
- [ ] erledigt
- **Modell:** Opus · **Braucht:** N.1 (Gerüst-Antworten), fertig nach N.6 · **Dateien:**
  `crates/torroworkplace/src/ui/{mod,sync,persona,bots,accounts}.rs` (`sync.rs` neu)
- Neue Seite „Sync“ (`Section::Sync`, `torroworkplace ui --section sync`): Schalter, Nextcloud-Konto, Bereiche
  (Assistant, Settings), Stand („Last synced 2 min ago“, Fehler), „Sync now“.
- „Computers“: dieser Rechner (umbenennen), andere (bestätigen mit Schlüssel-Kurzform, entfernen),
  „Remove this computer from sync“.
- „Conflicts“: je Konflikt beide Fassungen nebeneinander mit Rechner und Zeit, „Keep this“ je Seite; bei Texten
  „Edit“ öffnet die Persona-Seite bzw. den Bot-Editor. Hinweis-Zeile auf Persona-Seite und im Bot-Editor, wenn dort
  ein Konflikt offen ist; Hinweis auf der Kontoseite von Nextcloud („Sync assistant and settings“).
- **API/MCP:** nur Requests aus N.1.
- **Abnahme:** `torro-design-review` in allen Größenklassen (mit `--demo`: zwei Rechner, ein Konflikt); Texte über
  `tr()`, Übersetzungen im Bericht.

### N.8 Von Hand geänderte Dateien erkennen
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** N.4 · **Dateien:** `crates/torroworkplace/src/sync/local.rs`
- Je Takt Prüfsumme (SHA-256) von `memory.md`, `persona.md`, `profile.json` und den Bot-Texten gegen den Stand des
  letzten Abgleichs (`<state>/sync/hashes.json`); bei Abweichung Notizen über `parse_notes` vergleichen (neue,
  geänderte, entfernte nach Kennung) und Texte als Ganzes, daraus Einträge.
- **Oberfläche/API/MCP:** keine.
- **Abnahme:** Tests: Zeile von Hand ergänzt → ein Eintrag; Zeile entfernt → Grabstein; unverändert → nichts; eine
  Datei, die der Abgleich selbst geschrieben hat, erzeugt keinen Eintrag.
