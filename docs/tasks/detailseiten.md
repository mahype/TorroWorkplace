# Aufgaben: Detailseiten der Einstellungen (DS.1–DS.8)

Konzept: [settings.md](../settings.md) (Einstellungsseiten), [mail.md](../mail.md) (Mail-Speicher, Sync),
[api.md](../api.md). Issue #314, geplant am 2026-10-26. Entwurf: Design-Canvas „Torro Settings Redesign“ (im Issue
verlinkt; Mail-Konto Übersicht, Ordner, Schreiben, MCP-Client, schmale Kachel, Bausteine). Regeln für die Arbeit:
[CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`, Oberfläche: Skill `torro-design`. Wer einen
Request anfasst, lädt `torro-api`; nach DS.1 prüft `torro-api-review`.

Umfang (in der Planung entschieden): #314 baut die Bausteine, die Formate, die neuen Mail-Daten, die Seite eines
Mail-Kontos und „Storage & index“. Die übrigen Seiten der Tabelle im Issue (Konten und KI, API-Keys und MCP-Clients,
MCP-Server, Kategorien und Regeln, Assistent, Werkzeuge) folgen in eigenen Issues auf diesen Bausteinen.

```
DS.1 (Mail-Daten im Dienst) ───────────────────────────────┐
DS.2 (Formate: Datum, Größe, Zahl) ────────────────────────┤
DS.3 (Kopf, Reiter, Offene Frage) ──┬── DS.5 (Host) ───────┼── DS.6 (Mail-Konto) ── DS.7 (Storage & index) ── DS.8 (Doku)
DS.4 (Kennzahlen, Anteilsbalken, ───┘                      │
      Einstellungspaar, Speicherleiste) ───────────────────┘
```

Reihenfolge in einem Strang: DS.1, DS.2, DS.3, DS.4, DS.5, DS.6, DS.7, DS.8.

**Festlegungen für alle DS-Aufgaben:**

- **Seitenaufbau** (Bausteine in `torro_gui::widgets`, Nummern wie im Issue): Kopf (1) · Reiter (2), nur ab mehr als
  drei Gruppen, „Overview“ zuerst · Offene Frage (6) direkt unter den Reitern, nur wenn etwas wartet · Kennzahlen (3)
  · Anteilsbalken (4) · Tabelle (5) · Einstellungspaare (7) · Speicherleiste (8) nur auf Seiten, die nicht selbst
  speichern, und nur solange etwas geändert ist. Keine Statistik als Einstellungszeile.
- **Reiter** sind ein eigener Baustein mit Unterstrich (in der Planung entschieden), nicht `segmented`: `segmented`
  wechselt die Ansicht derselben Daten, Reiter wechseln den Teil eines Objekts. Gewählt: `TEXT`, `body_strong`, Strich
  2 px in `TEXT` unten; sonst `TEXT_2`. Nie Rot.
- **Größen** (in der Planung entschieden): eine Funktion, Basis 1024, Einheiten B, KB, MB, GB, TB, drei gültige
  Ziffern („60.9 MB“, „512 MB“, „2.36 GB“), Dezimalzeichen nach Sprache (de „60,9 MB“). Basis 1024, damit ein
  IMAP-Kontingent (in KiB gemeldet) als „10 GB“ erscheint, wie es der Anbieter verkauft.
- **Daten**: Ortszeit; „Today 15:39“, „Yesterday 09:12“, „5 Oct“ im laufenden Jahr, sonst „5 Oct 2020“; der genaue
  Wert („Monday, 5 October 2020, 15:39:12“) als Tooltip. Zahlen mit Tausendertrenner nach Sprache („4,796“ / „4.796“).
- **Kontingent ohne `QUOTA`** (in der Planung entschieden): Kachel „On the server“ mit der Summe der Ordnergrößen,
  ohne „of …“; der Server-Balken zeigt dann nur die Ordner, ohne „Free“.
- Neue Wörter englisch mit `tr()`/`trf()`, deutsch in `crates/torro-core/src/i18n/de.rs` (ein Strang, deshalb trägt
  jede Aufgabe ihre Übersetzungen selbst ein). Nur Tokens, keine Zahlen in Apps (`scripts/design-check.sh`).

---

### DS.1 Mail-Daten im Dienst: Kontingent, Ordnergrößen, Mail je Monat, Originale je Ordner
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-ipc/src/mail.rs`, `crates/torro-ipc/src/api.rs`,
  `docs/api-reference.md`, `crates/torro-mail-imap/src/imap.rs`, `crates/torro-mail-imap/tests/imap.rs`,
  `crates/torroworkplace/src/mail/{store,sync,read}.rs`, `crates/torro-settings-ui/src/demo.rs`
- **Oberfläche:** Mail › Accounts › Konto (DS.6) · **API:** `Request::MailAccountStats` · `mail.account_stats` ·
  Zugriff Read, Bereich Mail (unverändert, nur neue optionale Felder) · **MCP:** `mail_account_stats` (gibt die
  Antwort unverändert weiter, nachgesehen: kein Sonderfall in `mcp.rs`).
- `torro_ipc::mail`: neue Felder, alle `#[serde(default)]`:
  - `MailAccountStats::quota: Option<MailQuota>` mit `MailQuota { used_bytes: u64, limit_bytes: u64, checked: DateTime<Utc> }`.
  - `MailAccountStats::months: Vec<MailMonthCount>` mit `MailMonthCount { year: i32, month: u32, messages: u64,
    cached_bodies: u64 }`, aufsteigend, nur Monate mit Mail, gezählt nach `messages.date` in UTC.
  - `MailAccountStats::originals: u64`, `original_bytes: u64` (Originale mit `gone_at IS NULL`).
  - `MailFolderStats::server_bytes: Option<u64>` (Summe `messages.size`; `None`, solange einer Mail des Ordners die
    Größe fehlt), `originals: u64`, `original_bytes: u64` (über `originals.folder_path` = `Folder::path`).
- Mail-Speicher `store.rs`: `SCHEMA_VERSION` 12; `messages.size INTEGER` (NULL für Zeilen von vorher), nur
  hinzufügen, wo sie fehlt (`has_column`, wie V6–V8). `MailStore::set_size(folder, uid, bytes)` neben
  `set_internal_date`; `MailStore::uids_without_size(folder, limit) -> Vec<u32>`; Kontingent als JSON unter
  `cache_control.key = 'quota'` (`set_quota(Option<&MailQuota>)`, `None` löscht). `account_stats` liest alles mit.
- IMAP `imap.rs`: `Session::quota_root(&mut self, mailbox: &str) -> Result<Option<Quota>, ImapError>` mit
  `GETQUOTAROOT` (RFC 9208), nur wenn `capabilities().has("QUOTA")`, sonst `Ok(None)`; liest die Ressource
  `STORAGE` aus `* QUOTA` (Einheit 1024 Bytes). `pub struct Quota { pub used_kib: u64, pub limit_kib: u64 }`.
- Sync `sync.rs`: `sync_folder` speichert `FetchedHeader::size` mit `set_size`; danach füllt es bis zu 2000
  fehlende Größen je Ordner und Lauf nach (`uids_without_size`, `fetch_backup_metadata` in Stapeln zu 500,
  `BackupMessage::bytes`), damit alte Konten ihre Größen ohne Last-Spitze bekommen. `sync_account` fragt einmal je
  Lauf `quota_root("INBOX")` und speichert es (`None` löscht ein altes Kontingent).
- Katalog `ACCOUNT_STATS`: Beschreibung nennt `quota` (nur wenn der Server QUOTA kann; Stand des letzten Syncs),
  `server_bytes` je Ordner (fehlt, bis alle Größen bekannt sind), `months` und die Originale; dazu: Kontingent und
  Größen sind Angaben des Servers vom letzten Sync, keine Live-Werte; `months` zählt nach Datum in UTC; Originale sind
  nur behaltene Originale, die noch auf dem Server liegen, und die Ordnerwerte müssen nicht auf den Kontowert
  aufgehen (umbenannte Ordner bis zum nächsten Sync). API-Referenz neu
  erzeugen (`TORRO_BLESS=1 cargo test -p torro-ipc`).
- Demo `demo.rs` (`mail_stats`): Kontingent 2.36 GB von 10 GB, Ordnergrößen, Originale, Monate über sechs Jahre;
  dazu ein dritter und vierter Ordner (Sent, Junk), damit die Tabelle und „other folders“ etwas zeigen.
- **Abnahme:** Tests: `quota_root` liest `* QUOTA "" (STORAGE 2475 10485760)` und gibt ohne `QUOTA` `None` ohne
  Befehl; Migration 11 → 12 behält Zeilen, `size` ist NULL; `account_stats` liefert `server_bytes` erst, wenn alle
  Größen da sind, Monate richtig gezählt, Originale je Ordner ohne verschwundene; Sync-Test mit dem Test-Server:
  Größen gespeichert, fehlende nachgefüllt, Kontingent gespeichert und ohne `QUOTA` gelöscht; alte JSON-Antwort ohne
  die neuen Felder liest sich weiter. `torro-api-review` gelaufen.

### DS.2 Formate: Größe, Zeitpunkt, Zahl
- [x] erledigt
- **Modell:** Haiku · **Braucht:** – · **Dateien:** `crates/torro-gui/src/units.rs` (neu), `crates/torro-gui/src/lib.rs`,
  `crates/torro-gui/src/datetime.rs`, `crates/torro-settings-ui/src/mail.rs`, `crates/torro-settings-ui/src/mail/backup.rs`,
  `crates/torro-core/src/i18n/de.rs`
- `torro_gui::units::format_bytes(u64) -> String` und `format_count(u64) -> String` nach den Festlegungen oben;
  Sprache aus `torro_core::i18n::lang()`.
- `torro_gui::datetime::format_moment(at: DateTime<Utc>, now: DateTime<Utc>) -> String` („Today 15:39“ …) und
  `format_exact(at: DateTime<Utc>) -> String` (Tooltip), beide in Ortszeit über `to_local`.
- In `torro-settings-ui` ersetzen: `accounts::byte_size` (mail.rs) und seine Aufrufer in `mail/backup.rs`, die
  ganzen MiB in `storage::page`, `format_date_value` und die übrigen `"%Y-%m-%d %H:%M UTC"` in mail.rs. Die
  Steuerzentrale (`accounts.rs`, `api.rs`, `workspaces.rs`) bleibt für die Folge-Issues.
- **Abnahme:** Tests mit festen Werten: 0 B, 1023 B, 1 KB, 60.9 MB, 512 MB, 2.36 GB, 10 GB aus 10485760 KiB; de mit
  Komma; „Today“, „Yesterday“, laufendes und früheres Jahr, Jahreswechsel; `format_count` 4,796 / 4.796. Kein
  `MiB` und kein `UTC` mehr in `torro-settings-ui/src/mail*` (grep). Übersetzungen („Today {time}“ …) in `de.rs`.

### DS.3 Bausteine: Kopf, Reiter, Offene Frage
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-gui/src/widgets.rs`, `crates/torro-gui/examples/gallery.rs`,
  `crates/torro-core/src/i18n/de.rs`
- `widgets::detail_head(lead, title, meta, status, actions)`: links eine Kachel (`source_tile`, `TileSize::Large`),
  Titel `typo::title()`, darunter eine Meta-Zeile `typo::meta()`/`TEXT_3` (einzeilig, „…“ mit Tooltip), rechts
  optional `state_pill` und die Knöpfe des Aufrufers (eine `secondary`-Aktion, dann `icon_button(Ellipsis, "More")`,
  dessen Menü der Aufrufer mit `widgets::floating` öffnet). Unter `size::form_max()` Breite rutschen Pille und
  Knöpfe unter die Meta-Zeile.
- `widgets::tabs(labels: Vec<(String, Option<u64>)>, selected: usize, on_select: fn(usize) -> M)`: Höhe
  `size::control_lg()`, Abstand `space::xs()`, Zahl rechts im Reiter in `typo::meta()`/`TEXT_3` tabellarisch, eine
  `LINE` unter der ganzen Reihe. Passt die Reihe nicht (`tabs_width`), wird sie zu einer Auswahlliste
  (`pick_list` mit `style::pick_list`), nie waagerecht scrollend.
- `widgets::question_card(mark, title, detail, actions)`: Fläche `RAISED`, Rand `LINE_STRONG`, Radius `radius::sm()`,
  Innenrand `layout::panel_padding()`, Icon in Attention, Titel `body_strong`, Satz in `TEXT_2`, Knöpfe rechts bzw.
  darunter (umbrechend); mehrere Fragen stehen untereinander mit `size::card_gap()`.
- Galerie: neue Seite `--detail` mit Kopf (mit und ohne Pille, schmal), Reitern (breit, mit Zahl, als Auswahlliste)
  und Offener Frage; Fenstergröße wie `--blocks`.
- **Abnahme:** Tests: `tabs_width` wächst mit Wort und Zahl; Reiter melden ihren Index; Kopf baut ohne Pille und
  ohne Knöpfe. Galerie in Standard, Compact und Console, hell und dunkel angesehen. Übersetzung „More“ prüfen.

### DS.4 Bausteine: Kennzahlen, Anteilsbalken, Einstellungspaar, Speicherleiste, ruhiger Zweig
- [x] erledigt
- **Modell:** Opus · **Braucht:** DS.3 (Galerie-Seite) · **Dateien:** `crates/torro-gui/src/widgets.rs`,
  `crates/torro-gui/examples/gallery.rs`, `crates/torro-core/src/i18n/de.rs`
- `widgets::stat_grid(tiles)`: 2–4 `stat_tile` in einer Reihe, darunter 2 Spalten, in Mini eine (`responsive`, wie
  `rights::view` es auf Einstellungsseiten schon tut). `stat_tile` bleibt unverändert.
- `widgets::share_strip(parts: Vec<SharePart>, rest: Option<(String, u64)>)` mit `SharePart { label, value: u64,
  shown: String, color: ChartColor }`: ein Balken `space::s()` hoch, Segmente in ihrer `ChartColor` mit 2 px Fläche
  dazwischen, `rest` („Free“) als Spur in `LINE_STRONG`; darunter die Legende (Punkt, Wort, `shown`, Anteil in %,
  umbrechend), jeder Wert auch ohne Farbe lesbar. Höchstens vier Reihen und `ChartColor::Other`. Der vorhandene
  `share_bar` (ein Anteil, neutral) bleibt.
- `widgets::setting_pair(title, why, control)`: links Titel `body_strong` und Satz `meta`/`TEXT_2`, rechts das
  Bedienelement, Spalten 1 : 1.6, oben bündig; unter `size::form_max()` untereinander (`responsive`). Mehrere Paare
  trennt eine `LINE`, Abstand `layout::section_gap()`.
- `widgets::save_bar(summary, discard: Option<M>, save: Option<M>, busy: bool)`: `floating_bar` mit Satz
  („Signature changed“) in `TEXT_2`, „Discard“ (`ghost`) und „Save“ (`primary`, beschäftigt `busy_button("Saving…")`).
- `widgets::sidebar_branch_quiet(rows)`: wie `sidebar_branch`, die Linie in `LINE_STRONG` aus der Stilfunktion
  (für Objekte unter einem Abschnitt, die keine eigene Farbe haben). `sidebar_branch` bleibt unverändert.
- Galerie `--detail`: alle Bausteine dazu, der Anteilsbalken mit und ohne „Free“, mit fünf Teilen (der fünfte als
  Other), die Speicherleiste über einer Beispielseite.
- **Abnahme:** Tests: Anteile summieren auf die Breite, 0 Gesamt zeichnet nur die Spur; fünfter Teil wird Other;
  Prozent gerundet, „<1 %“ für kleine Teile. Galerie in drei Designs, hell und dunkel, breit und schmal angesehen.
  Übersetzungen („Free“, „Discard“, „Saving…“) in `de.rs`.

### DS.5 Einstellungs-Host: Objekt als Kind des Abschnitts, Platz für die Speicherleiste
- [x] erledigt
- **Modell:** Opus · **Braucht:** DS.3, DS.4 · **Dateien:** `crates/torro-settings-ui/src/host.rs`
- `Host::list_rows`: Ist eine Unterseite offen (`Page::sub_page`), steht ihr Name als `sidebar_child` (mit dem Icon des
  Abschnitts, aktiv) in `sidebar_branch_quiet` unter dem aktiven Abschnitt; der Abschnitt selbst ist dann nicht
  mehr aktiv, ein Klick auf ihn führt zur Liste zurück (die `back`-Nachricht der Unterseite). `menu_card` zeigt das
  Kind als `MenuItem::nested()`.
- `Page::save_bar(Element)`: `page_area` legt sie mit `widgets::over_bar` über die scrollende Seite und gibt der
  Seite `floating_bar_clearance(1)` unten Luft. Ohne Aufruf ändert sich nichts.
- Kopf und Reiter stehen im Inhalt der Seite, nicht in der Kopfleiste; die Kopfleiste behält den Pfad.
- **Abnahme:** Tests: mit Unterseite enthält die Liste das Kind und der Abschnitt führt zurück; ohne bleibt sie wie
  bisher; eine Seite mit Speicherleiste baut in allen drei `Form`s. Bestehende Host-Tests grün.

### DS.6 Mail › Accounts › Konto: Kopf, Reiter Overview · Writing · Folders · Backup
- [ ] erledigt
- **Modell:** Opus · **Braucht:** DS.1, DS.2, DS.4, DS.5 · **Dateien:** `crates/torro-settings-ui/src/mail.rs`,
  `crates/torro-settings-ui/src/mail/backup.rs` (nur Formate, falls DS.2 etwas offen ließ), `crates/torroinbox/src/app.rs`
  (Demo-Zustände), `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** diese Seite · **API:** liest `mail.account_stats`, schickt wie bisher `accounts.update_mail_account`
  und `accounts.sync` · **MCP:** nichts Neues (reine Darstellung).
- `accounts::State` bekommt den gewählten Reiter (beim Öffnen eines Kontos Overview); `folders_expanded` und
  „Show folders“ entfallen.
- **Kopf:** Buchstaben-Kachel, Adresse, Meta „Name · IMAP host:port · SMTP host:port“, Pille „Synced {moment}“ (ok)
  bzw. das Sync-Problem (Fehler), Aktion „Sync now“ (`Request::Sync`, synct alle Konten; Tooltip sagt das), „…“ mit
  „Storage & index“ (`Action::Open("storage")`) und „Server settings“ (`Action::ControlCentre("accounts")`).
- **Overview:** Offene Fragen (offene Ordnerrollen → „Choose folders“ öffnet Folders; Sync-Problem) · Kennzahlen Mail
  (Zahl, „{n} unread“), On the server (Kontingent „of 10 GB · 24 %“ oder Ordnersumme), On this device
  (`database_bytes`, „{n} texts offline“), Searchable (Anteil `indexed_full`, „{n} with full text“) · „Storage“ mit
  zwei `share_strip`: Server (drei größte Ordner nach `server_bytes`, „{n} other folders“, „Free“) und Gerät (Search
  index, Mail texts, Originals, Headers & folders = Rest von `database_bytes`, nie unter 0; ohne `search_index_bytes`
  heißt der Rest „Index, headers & folders“; Attachments als Zeile darunter);
  Verweis „Manage in Storage & index“ · „Mail over time“: `column_chart` je Jahr (je Monat, wenn alles in zwei Jahren
  liegt), Reihen „Readable offline“ und „Headers only“ aus `months`, darunter „Oldest {moment} · newest {moment}“ und
  das Offline-Fenster ab `oldest_cached_body` mit „Change“ → Storage & index · „Search index“ als `share_strip` (Full
  text, Partial or unverified, Awaiting, Text unavailable), „Last run {moment}“ · „Largest folders“: `data_table` der
  fünf größten (Folder, Mail, Unread, Share, On server), „All {n} folders“ öffnet Folders.
- **Writing:** Sender name, Signature, Write new mail as als `setting_pair`; Name und Signatur speichern über die
  Speicherleiste (`Page::save_bar`, nur mit Änderung; Discard setzt den Entwurf zurück); das Format sendet sofort
  wie bisher. Der Inline-„Save“ entfällt.
- **Folders:** offene Rollen als `question_card` mit „Use {folder}“ je Kandidat (sendet wie `PickFolder`), dann die
  volle `data_table`: Folder, Mail, Unread, Offline texts, Indexed, On server, Originals; Summenzeile; Ordner ohne
  `server_bytes` zeigen „–“ mit Satz unter der Tabelle, dass Größen beim nächsten Sync kommen.
- **Backup:** `backup::view` unverändert.
- Demo `torroinbox` `Open`: `account` (Overview), `account-folders`, `account-writing` (mit geändertem Namen, also
  Speicherleiste), `account-question` (offene Rolle und Sync-Problem); wie die `backup`-Zustände öffnen sie die
  Einstellungen beim ersten Konto.
- **Abnahme:** Tests: ein Reiter-Wechsel schickt nichts; „Sync now“ schickt genau `Request::Sync`; Save schickt nur
  das Geänderte (bestehender Test angepasst), Discard nichts; Kontingent mit und ohne `quota`; „{n} other folders“
  ab vier Ordnern; jede Seite baut mit und ohne Statistik (Leerzustand „Statistics are not available yet.“).
  Sichtprüfung der vier Demo-Zustände in allen Größen und drei Designs (`torro-design-review`). Übersetzungen in `de.rs`.

### DS.7 Mail › Storage & index: nur Strategie und Wartung
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** DS.2, DS.6 · **Dateien:** `crates/torro-settings-ui/src/mail.rs` (`storage`),
  `crates/torro-core/src/i18n/de.rs`
- Die Gruppe „Local storage & index“ entfällt. An ihrer Stelle eine Zeile „Numbers of this account“ mit „Show
  account“, die Mail › Accounts mit diesem Konto im Reiter Overview öffnet: `storage::Message::ShowAccount(id)` fängt
  `mail::update` ab, öffnet das Konto in `state.accounts` (wie `accounts::Message::Open`) und gibt
  `Action::Open("accounts")` zurück.
- „Current job“, Schätzungen und Anhänge mit `format_bytes`/`format_count`; Strategien und Wartung bleiben, wie sie sind.
- **Abnahme:** Tests: die Seite zeigt keine der Zahlen aus `MailAccountStats` außer Schätzungen und Job; „Show
  account“ landet auf dem Konto. Sichtprüfung `--settings storage` in allen Größen.

### DS.8 Doku
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** DS.1–DS.7 · **Dateien:** `docs/settings.md`, `docs/mail.md`,
  `.claude/skills/torro-design/SKILL.md`, `docs/tasks/detailseiten.md`
- `settings.md`: „Keine Reiter“ gilt für die Abschnitte; Detailseiten (dritte Ebene) folgen dem Aufbau oben, mit
  Reitern ab mehr als drei Gruppen und der Speicherleiste als Ausnahme vom automatischen Speichern.
- Design-Skill, Abschnitt 5: die neuen Bausteine (`detail_head`, `tabs`, `question_card`, `stat_grid`,
  `share_strip`, `setting_pair`, `save_bar`, `sidebar_branch_quiet`), die Formate (`units`, `format_moment`) und die
  Galerie `--detail`; „Einstellungsseiten: nur `widgets::group`“ an das Muster anpassen.
- `mail.md`: Kontingent und Größen je Ordner (wann geholt, `messages.size`, Nachfüllen).
- **Abnahme:** Doku widerspricht dem Code nicht; Aufgaben abgehakt.
