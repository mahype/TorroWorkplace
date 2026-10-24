# Aufgaben: Schrift im Dropdown, 25 freie Schriften, Schriften des Systems (SR.1–SR.8)

Konzept und Format: [design-pakete.md](../design-pakete.md) (Schriften seit #189, Größenausgleich und Ziffern seit
L.19). Issue #299. Regeln für die Arbeit: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`,
Oberfläche: Skill `torro-design`, Requests: Skill `torro-api`, danach `torro-api-review`.

> **In der Planung entschieden (2026-10-24):** alle 25 Schriften der Tabelle im Issue, darin die vier aus L.24 (L.24
> zieht hierher); Systemschriften mit ungleich breiten Ziffern bekommen Inter Tabular als Ziffernschrift statt
> abgelehnt zu werden; die Schriften von Torro stehen im Dropdown in den Untergruppen Sans, Serif und Mono (neuer
> Schlüssel `kind` in `font.toml`); das Dropdown hat oben ein Suchfeld.

```
SR.1 Kern ── SR.2 Dienst/API/MCP ── SR.3 Baustein Menü mit Suche ── SR.4 Dropdown ── SR.5 Sans 1 ── SR.6 Sans 2 ── SR.7 Serif, Mono ── SR.8 Abschluss
```

Nacheinander in dieser Reihenfolge: SR.2 braucht das Modell aus SR.1, SR.4 den Baustein aus SR.3 und `FontInfo` aus
SR.2; SR.5–SR.7 ändern dieselbe Liste `FONTS` in `designs.rs` und denselben Test.

**Neue Abhängigkeit** (SR.1, über `[workspace.dependencies]`): `fontdb = "0.23.0"` mit `default-features = false`
und den Features `std`, `fs`, `memmap`, `fontconfig` – die Fassung und Features, die cosmic-text (Iced 0.14) schon
zieht; `Cargo.lock` bekommt kein neues Paket. Nachgesehen im Versuch zur Planung (macOS, Release): alle
Systemschriften auflisten und je Familie den regulären Schnitt prüfen dauert rund 100 ms (317 Familien, 123 mit
lateinischer Schrift, 85 davon mit gleich breiten Ziffern, 4 variabel).

**Ids** (SR.1): `system` ist der Eintrag „System“ (Oberflächenschrift des Betriebssystems, beim Laden aufgelöst),
`system:<Familie>` eine installierte Schrift (`system:Avenir Next`). Ordner-Ids bleiben `a-z0-9-`, deshalb kann keine
Ordner-Id mit `system:` kollidieren; `system` selbst wird wie `torro` reserviert (`is_valid_id`). Gespeichert wird nur
die Id in `Settings::font`; das Format der Einstellungen ändert sich nicht.

**Drei Wege** (keine neue Funktion, kein neuer Request):

- **Oberfläche:** Steuerzentrale › Appearance › Font als Dropdown mit Suchfeld (SR.4).
- **API:** `Request::ListFonts` · `settings.list_fonts` · Read (gibt es) – `FontInfo` neu mit `origin` und `kind`;
  `Request::SetSetting` · `settings.set_setting` `font` · Admin (gibt es) – nimmt auch `system` und
  `system:<Familie>`.
- **MCP:** `settings_list_fonts`, `settings_set_setting` (gibt es; Beschreibung kommt aus dem Katalog).
- Keine Geheimnisse; das Auflisten installierter Schriften ist kein Recht, das ein Agent verwalten müsste.

## Gemeinsame Regeln für SR.5–SR.7 (mitgelieferte Schriften)

- **Ordner** `fonts/<id>/` mit `font.toml`, den Schriftdateien (TTF, feste Schnitte Regular, Medium, SemiBold, Bold,
  soweit die Schrift sie hat) und `LICENSE.txt` = die `OFL.txt` der Schrift wörtlich, Copyright-Zeilen unverändert.
- **Quellen** nur `github.com/google/fonts` oder die Releases der Hersteller (vom Nutzer erlaubt, #256). Feste
  Schnitte bevorzugt aus den Releases der Hersteller; gibt es nur eine variable Datei, werden feste Schnitte daraus
  erzeugt (fontTools `instancer`) – das ist eine **veränderte Datei**.
- **Reserved Font Name:** Eine veränderte Datei (Schnitt aus variabler Schrift, eingefrorene Ziffern, Teilmenge) einer
  Schrift mit RFN bekommt einen neuen Familiennamen ohne den RFN (Muster „Commitr Figures“, L.11); betroffen sind
  Source Sans 3, Source Serif 4, Source Code Pro, IBM Plex Mono, Merriweather, Lora. Unveränderte Dateien behalten
  ihren Namen. Steht in einer `OFL.txt` ein RFN, der nicht in der Tabelle unten steht: **anhalten und fragen**.
- **Ziffern** wie in L.5: Sind die Standardziffern gleich breit (alle zehn Vorschübe gleich in jedem Schnitt), bleibt
  `numbers` die Textschrift. Sonst eine eigene Ziffernschrift `<Name> Figures` (Regular und Medium) mit eingefrorenem
  `tnum` (fontTools, opentype-feature-freezer wie in L.11); ihr Familienname unterscheidet sich immer von der
  Textschrift, sonst kämen sich die Schnitte in Iced in die Quere. Breiteste Ziffer ≤ `DIGIT_MAX_EM`.
- **Größenausgleich:** von selbst aus der x-Höhe; wirkt er neben Inter falsch, `size_adjust` setzen und im Kommentar
  begründen.
- **`font.toml`:** Kopfkommentar mit Quelle (URL, Version/Release, Abrufdatum), Lizenz, RFN, was verändert wurde und
  wie die Ziffern geprüft sind; `kind = "sans" | "serif" | "mono"`; `[about]` mit neutralem `name` (keine Firma:
  „Plex Mono“ statt „IBM Plex Mono“), `version = "1.0.0"`, `author = "Torro"`, `description` englisch, ein Satz.
- **Einbetten:** je Schrift ein `Example` in `FONTS` (`crates/torroworkplace/src/designs.rs`) mit allen Dateien,
  alphabetisch nach den vorhandenen; die erwarteten Ids in `every_example_loads_without_reasons_in_its_folders`
  nachziehen. `the_embedded_files_are_the_whole_example` muss grün sein. `install_new` legt neue Ids in vorhandenen
  Datenordnern von selbst nach (nachgesehen).
- **Doku:** je Schrift eine Zeile in der Liste „Mitgelieferte Schriften“ in `docs/design-pakete.md` (SR.1 legt die
  Tabelle an): Id, Familien, Art, Quelle, Lizenz, RFN, verändert ja/nein, Ziffern.
- **Prüfen:** `design::load_font` lädt jede ohne Gründe; Sichtprüfung `torroworkplace ui --demo --open design-fonts
  --font <id>` hell und dunkel im Design Standard.

| Id | Familie(n) | Art | RFN |
|---|---|---|---|
| `hyperlegible` | Atkinson Hyperlegible Next | sans | – |
| `source` | Source Sans 3, `heading` Source Serif 4 | sans | „Source“ |
| `nunito` | Nunito | sans | – |
| `open-sans` | Open Sans | sans | – |
| `noto-sans` | Noto Sans | sans | – |
| `roboto` | Roboto | sans | – |
| `work-sans` | Work Sans | sans | – |
| `manrope` | Manrope | sans | – |
| `dm-sans` | DM Sans | sans | – |
| `public-sans` | Public Sans | sans | – |
| `lexend` | Lexend | sans | nur „RevReading Lexend“ |
| `fira-sans` | Fira Sans | sans | – |
| `geist` | Geist | sans | – |
| `rubik` | Rubik | sans | – |
| `cantarell` | Cantarell | sans | – |
| `literata` | Literata | serif | – |
| `merriweather` | Merriweather | serif | „Merriweather“ |
| `lora` | Lora | serif | „Lora“ |
| `newsreader` | Newsreader | serif | – |
| `jetbrains-mono` | JetBrains Mono | mono | – |
| `fira-code` | Fira Code | mono | – |
| `plex-mono` | IBM Plex Mono (Name „Plex Mono“) | mono | „Plex“ |
| `source-code-pro` | Source Code Pro | mono | „Source“ |
| `geist-mono` | Geist Mono | mono | – |
| `inconsolata` | Inconsolata | mono | – |

`jetbrains-mono` ist die Id, die Console schon vorschlägt (`[about] font`); sie muss so heißen.

### SR.1 Kern: Schriften des Systems, `kind`, `Face::System`
- [ ] erledigt
- **Modell:** Opus · **Dateien:** `Cargo.toml` (Wurzel), `crates/torro-core/Cargo.toml`,
  `crates/torro-core/src/design.rs`, `crates/torro-core/src/design/{font,files,error}.rs`, neu
  `crates/torro-core/src/design/system.rs`, `crates/torro-gui/src/design.rs`, `crates/torroworkplace/src/tools/pdf.rs`,
  `fonts/{plex,commitr,ribbon,channelr,guildr,quattro}/font.toml`, `docs/design-pakete.md`
- **`kind`:** `pub enum FontKind { Sans, Serif, Mono }` (serde lowercase, `Deserialize`/`Serialize`) in
  `torro_core::design`; `font.toml` bekommt den optionalen Schlüssel `kind` auf oberster Ebene (`known`,
  `FontSetFile`), `FontSet` das Feld `kind: Option<FontKind>`; `FontSet::standard()` ist `Sans`. Die sechs vorhandenen
  Pakete bekommen `kind = "sans"`.
- **`Face::System`:** neue Variante „gezeichnet über den Familiennamen aus den Schriften des Systems, ohne Bytes“.
  `torro_gui::design::font_files` und `font_file` liefern für sie nichts (Iced lädt die Systemschriften selbst,
  cosmic-text `FontSystem::new_with_fonts` → `load_system_fonts`, nachgesehen). Damit findet `PdfFonts::of` keine
  Datei und das PDF nimmt Inter (`PdfFonts::standard`, nachgesehen): Eine Systemschrift wird nie in ein PDF
  eingebettet; in `pdf.rs` nur ein Test.
- **`system.rs`:** `SystemFonts` als eine Abtastung (`SystemFonts::scan()` über `fontdb::Database::load_system_fonts`,
  für Tests `SystemFonts::from_database(fontdb::Database)`), daraus `list(&self) -> Vec<Listed<FontSet>>` (erst
  `system`, dann die Familien nach Namen) und `load(&self, id) -> Result<FontSet, Vec<DesignError>>`. Öffentlich
  zusätzlich `list_system_fonts()` (eine Abtastung). Regeln je Familie, geprüft am Schnitt mit Gewicht 400 und Stil
  normal (x-Höhe über `files::font_info`; Zeichenabdeckung und Gleichheit der zehn Ziffernvorschübe mit `ttf-parser`
  in `system.rs`, denn `font_info` kennt nur die breiteste Ziffer):
  - aufgenommen nur Familien, deren Name nicht mit `.` beginnt, höchstens 64 Zeichen hat und keine Steuer- oder
    Formatzeichen enthält (wie `approvals::is_format`; der Name kommt aus einer fremden Datei und wird Teil der Id),
    mit einem normalen Schnitt und mit den Zeichen `A–Z a–z ä ö ü ß é` im regulären Schnitt; alles andere erscheint
    gar nicht (Symbol-, Emoji-, nicht lateinische);
  - kein Schnitt mit Gewicht 400 → ungültig mit Grund (neue `DesignError`-Variante, Text englisch über `tr`);
  - Schnitte: `regular`/`medium`/`semibold`/`bold` nur, wo ein normaler Schnitt genau dieses Gewicht hat, sonst
    `Font::nearest`. Ist der reguläre Schnitt variabel mit Achse `wght`, gelten Regular, Medium und SemiBold als
    vorhanden, Bold nicht (Versuch: cosmic-text zeichnet SF bei 500/600 aus der Achse, fällt bei 700 auf Menlo);
  - `numbers`: gleich breite Ziffern im regulären Schnitt → die Familie selbst, sonst die eingebaute Inter Tabular
    (`FontSet::standard().fonts.numbers`, gemessen mit `BUILTIN_WIDEST_FIGURE`) (Entscheidung);
  - Größenausgleich wie bei Paketen (`size_adjust_for` aus der x-Höhe); `rules::font` gilt;
  - `license` bleibt leer (`String::new()`), `about.name` = Familie, `kind` = `None`;
  - Gründe (`DesignError`) einer Systemschrift nennen nur die Familie, **nie einen Dateipfad** (er verriete
    Benutzername und Home-Verzeichnis an Agenten); auch Fehler von fontdb und ttf-parser werden so umgeschrieben.
  - Ins Protokoll (tracing) nur Anzahl und Dauer der Abtastung, keine Familiennamen, keine Pfade.
- **„System“:** `system` löst auf die erste vorhandene Familie einer festen Liste je Plattform auf (macOS: `System
  Font`; Windows: `Segoe UI Variable Text`, `Segoe UI Variable`, `Segoe UI`), auf Linux auf die Familie von
  `fc-match -f '%{family[0]}' sans-serif` (Befehl, `#[cfg(target_os = "linux")]`); nichts gefunden → `NotFound`, die
  Apps zeichnen mit dem Standard. `about.name` ist „System“; die aufgelöste Familie steht in `fonts.text.family`.
- **`load_font`:** Ids `system` und `system:…` gehen an eine frische `SystemFonts::scan()`; fehlt die Familie →
  `[DesignError::NotFound { part: Part::Font, … }]`, damit `torro_gui::design::part` still den Standard nimmt.
  `list_fonts(dir)` bleibt bei den Ordnern (Signatur fest). `is_valid_id` lehnt `system` ab. Ein Design kann weiterhin
  keine Systemschrift vorschlagen (`pack.rs`, Prüfung des Vorschlags mit `is_valid_id || STANDARD_ID`): so bleibt es,
  damit `approvals::Facts::design` unverändert bleibt.
- **Nichts wird kopiert:** `system.rs` liest Dateien nur zum Prüfen (memmap über fontdb) und schreibt nie.
- **Doku** (`docs/design-pakete.md`): Format-Tabelle um `kind`; neuer Abschnitt „Schriften des Systems“ (mitnutzen,
  nicht weitergeben; Ids; Filter; Regeln oben; Fallback auf den Standard; kein PDF); Abschnitt „Mitgelieferte
  Schriften (Vorschlag)“ wird „Mitgelieferte Schriften“ mit leerer Tabelle (Spalten wie in den Regeln für SR.5–SR.7)
  und den sieben vorhandenen.
- **Abnahme:** Die Gewichtsregel (variabel mit `wght` → Regular, Medium, SemiBold, kein Bold) als reine Funktion
  getestet, weil das Repository keine variable Testschrift hat. Tests in `system.rs` mit einer Datenbank aus den
  Testschriften (`testdata::font_file`, kein echtes System): Filter, fehlendes Regular mit Grund, Gewichte und
  `nearest`, Ziffern → Inter Tabular (Mona Sans hat ungleich breite Ziffern, Plex gleich breite), `system:Fehlt` →
  `NotFound`, `system` reserviert, Familie mit Steuerzeichen bzw. über 64 Zeichen nicht aufgenommen, kein Grund
  enthält einen Pfad. Test in `font.rs`: `kind` gelesen, unbekannter Wert abgelehnt. Test in `pdf.rs`:
  Schrift mit `Face::System` → `PdfFonts::standard()`. Test in `torro-gui`: `font_files` und `font_file` liefern für
  eine Systemschrift nichts.

### SR.2 Dienst, API, MCP: Herkunft und Art jeder Schrift, Systemschriften wählen
- [ ] erledigt
- **Modell:** Opus · **Braucht:** SR.1 · **Dateien:** `crates/torro-ipc/src/{lib,api}.rs`, `docs/api-reference.md`,
  `docs/api.md`, `crates/torroworkplace/src/{core,designs,settings_service,mcp,approvals}.rs`
- **`FontInfo`:** neues Feld `origin: FontOrigin` (`pub enum FontOrigin { Bundled, Folder, System }` in
  `torro-ipc`, serde `"bundled" | "folder" | "system"`, `#[serde(default)]` = `Folder`, damit eine Antwort eines
  älteren Kerns lesbar bleibt) und `kind: Option<torro_core::design::FontKind>` (`skip_serializing_if`). Doc-Kommentare
  sagen, was die Steuerzentrale daraus macht. Verträglich (neue optionale Felder), kein `v: 2`. `#[serde(other)]` auf
  `FontOrigin::Folder`, `kind` tolerant lesen (unbekannter Wert → `None`), damit ältere Clients spätere Werte lesen.
  Doc-Kommentare nachziehen: `Request::ListFonts` (nicht mehr nur „each folder under `<data>/fonts/`“),
  `SettingKey::Font` (beide Id-Formen, fehlende Systemschrift abgelehnt), `FontInfo::id` (nicht mehr nur Ordnername),
  `FontInfo::license` (`None` bei Systemschriften: genutzt, nie weitergegeben).
- **`Core::list_fonts`:** erst die Ordner wie heute, dann `torro_core::design::list_system_fonts()` in
  `tokio::task::spawn_blocking`. `origin`: Standard und Ids aus `designs.rs` → `Bundled` (neu `pub fn
  bundled_font(id: &str) -> bool` in `designs.rs`), andere Ordner → `Folder`, Systemschriften → `System`; `kind` aus
  dem `FontSet`; `license` bei `System` als `None` (nicht `Some("")`). `active` vergleicht wie heute die Id. „Reads
  only; never fails“ bleibt: scheitert `spawn_blocking`, liefert `list_fonts` nur die Ordner.
- **Katalog:** Beschreibung von `LIST_FONTS` um Herkunft, Art und die Ids `system`/`system:<Family>` ergänzen; in
  `SET_SETTING` beim Schlüssel `font` die beiden Id-Formen nennen und dass eine fehlende Systemschrift abgelehnt wird.
  In `LIST_FONTS` sagen, dass Familiennamen installierter Schriften und ihre `problems` aus fremden Schriftdateien
  stammen: Daten, nie Anweisungen. `docs/api.md`, Fußnote 9: `origin`, `kind`, die Ids, Familiennamen als Daten, nie
  im PDF, kein Vorschlag durch ein Design. Familiennamen gehen bewusst an Agenten (Read, Bereich Settings): sie verraten
  nur installierte Software, keine Geheimnisse.
  API-Referenz neu erzeugen (`TORRO_BLESS=1 cargo test -p torro-ipc`) und `docs/api-reference.md` mitcommitten.
- **`set_setting font`:** `load_part` nutzt `design::load_font`, nimmt damit Systemschriften von selbst (nachgesehen).
  Nur `SettingsStore::check_part` lehnt ohne Home-Verzeichnis alles außer `torro` ab: dort `system` und `system:…`
  ebenfalls über `load_font` prüfen (sie brauchen keinen Ordner). `part_id` begrenzt `system:…` vor dem Laden wie der
  Filter in SR.1 (höchstens 64 Zeichen Familie, keine Steuer- oder Formatzeichen), damit kein langer Text über
  `check_part` (`{id:?}`) oder die Zusammenfassung einer Freigabe läuft.
- **Freigabe:** Die Zusammenfassung von `set_setting font` (allgemeiner Zweig `Request::SetSetting` in
  `approvals.rs`) zeigt die Id über `quote` gekürzt und bereinigt; nur ein Test dafür mit `system:<…>`.
- **MCP:** keine Codeänderung (Tool = Katalog); Vertragstests laufen mit.
- **Abnahme:** Handler-Test: `list_fonts` nennt `origin` für Standard, ein eingebautes Beispiel, einen eigenen Ordner;
  Systemschriften stehen hinter den Ordnern (über einen Test-Hook oder indem nur geprüft wird, dass alle nach den
  Ordnern `System` sind – kein Test darf bestimmte installierte Schriften erwarten). `settings_service`: `system:Fehlt`
  abgelehnt mit Grund, `system:` mit Steuerzeichen bzw. zu langer Familie abgelehnt, ohne Home-Verzeichnis
  `system:…` geprüft statt abgelehnt. Serde-Test: `FontInfo` ohne `origin` liest sich als `Folder`, unbekannter
  `origin`/`kind` als `Folder`/`None`. `list_fonts` bei `System` ohne `license`. `torro-api-review` ohne Befund.

### SR.3 torro-gui: Baustein „Menü mit Suche“
- [ ] erledigt
- **Modell:** Opus · **Braucht:** SR.2 · **Dateien:** `crates/torro-gui/src/widgets.rs`,
  `crates/torro-gui/examples/gallery.rs`, `.claude/skills/torro-design/SKILL.md`
- Ein Baustein für lange Auswahlen in einem schwebenden Menü: oben ein Suchfeld (`style::text_input_flat`, Icon
  `Search`, Platzhalter „Search“ über `tr`, `size::control()` hoch), darunter eine `LINE`, darunter der rollende Inhalt
  des Aufrufers; Karte wie `general::scheme_menu` (Fläche und Rand eines Menüs, `style::MENU_INSET`, Höhe bis
  `size::panel()`). Der Aufrufer filtert selbst und reicht die Einträge herein; der Baustein bekommt Text, Nachricht
  beim Tippen und eine feste Id (`&'static str`), damit der Aufrufer beim Öffnen fokussieren kann. Leerer Treffer: eine
  Zeile „No matches“ in `TEXT_3`.
- Escape schließt (Aufrufer), Enter wählt den ersten Treffer (Aufrufer bekommt eine Nachricht `on_submit`).
- Galerie: ein Beispiel mit 40 Einträgen in zwei Abschnitten; SKILL.md Abschnitt 5 um den Baustein ergänzen.
- **Abnahme:** Test, dass der Baustein ohne Treffer „No matches“ zeigt und mit Treffern baut; Sichtprüfung in der
  Galerie hell und dunkel, in Standard und Console.

### SR.4 Steuerzentrale: Font als Dropdown
- [ ] erledigt
- **Modell:** Opus · **Braucht:** SR.3 · **Dateien:** `crates/torroworkplace/src/ui/{general,mod,launch}.rs`,
  `.claude/skills/torro-design/SKILL.md` (nur die Liste der `--open`-Zustände)
- `font_row`/`font_button` ersetzen durch eine Zeile wie `scheme_row`: Knopf `size::sidebar()` breit mit dem Namen der
  gezeichneten Schrift in ihrer eigenen Schrift und Chevron; der Satz zum Vorschlag (`FontHint`) bleibt unter „Font“.
- Das Dropdown (in `overlay`, neben dem des Farbschemas; nur eines offen) mit dem Baustein aus SR.3: Abschnitt
  **„Torro fonts“** (Origin `Bundled` und `Folder`) mit den Untergruppen „Sans“, „Serif“, „Mono“ und „Other“ (ohne
  `kind`), je der Standard zuerst, dann nach Namen; eine `LINE`; Abschnitt **„System fonts“** mit „System“ zuerst
  (Name über `tr("System")`, die aufgelöste Familie als Tooltip), dann die Familien nach Namen. Abschnitt- und
  Gruppentitel unterscheidbar (Abschnitt `typo::caption()` in `TEXT_2`, Gruppe `typo::caption()` in `TEXT_3`; die
  Sichtprüfung entscheidet), leere Gruppen und Abschnitte entfallen; die Suche filtert nach Name und Familie ohne
  Groß/Klein, Titel ohne Treffer entfallen.
- Eintrag wie `scheme_item`: Name in seiner eigenen Schrift (`torro_gui::design::font_of` mit `FontSet::apply` wie
  heute in `font_button`), Häkchen am gezeichneten, Punkt am vom Design vorgeschlagenen (Tooltip „Suggested for
  {design}“ bleibt), ungültige mit `CircleAlert`, Name in `TEXT_3`, Klick zeigt die Gründe unter der Gruppe
  (`Message::Problems`). Tooltip wie heute „name · version · author · licence“.
- Zustand: `fonts_open`, `font_query`, eigener `Anchor`; Nachrichten `ToggleFonts`, `CloseFonts`, `FontQuery(String)`,
  `FirstFont`; Escape (`escape`) schließt das offene Dropdown. Fokus aufs Suchfeld beim Öffnen: neue Variante
  `Action::Focus(&'static str)` in `ui/mod.rs`, dort als `operation::focus` ausgeführt.
- `list_looks`: Systemschriften (Origin `System`) nicht einzeln über `load_font` lesen, sondern aus **einer**
  `SystemFonts::scan()` (sonst je Eintrag eine Abtastung). Welche Einträge es gibt und ob sie gültig sind, kommt nur
  aus `settings.list_fonts`; die Abtastung dient nur dem Zeichnen der Probe, fehlt eine Familie darin, zeichnet die
  Probe den Standard.
- Demo (`launch.rs`, `demo_fonts`): Einträge mit `origin` und `kind`, dazu „System“ und zwei Systemschriften ohne
  `part` (zeichnen im Standard, damit die Bilder auf jedem Rechner gleich sind); neuer Zustand `--open design-fonts`
  (Dropdown offen) und `design-fonts-search` (Suche „mo“). Fußsatz der Gruppe: „More fonts go into the fonts folder.“
  ergänzen um die installierten Schriften.
- **Abnahme:** Tests: Gruppen und Reihenfolge, Suche filtert und blendet leere Titel aus, Enter wählt den ersten
  Treffer (`SetSetting font`), Escape schließt, nur ein Dropdown offen, ungültiger Eintrag zeigt Gründe. Sichtprüfung
  breit und schmal, hell und dunkel, in Standard und Console (`torro-design-review`).

### SR.5 Sans 1: Hyperlegible, Source, Nunito, Open Sans, Noto Sans, Roboto, Work Sans, Manrope
- [ ] erledigt
- **Modell:** Opus · **Braucht:** SR.4 · **Dateien:** `fonts/{hyperlegible,source,nunito,open-sans,noto-sans,roboto,
  work-sans,manrope}/`, `crates/torroworkplace/src/designs.rs`, `docs/design-pakete.md`
- Nach den gemeinsamen Regeln. Source: `heading` Source Serif 4 (Regular, Medium, SemiBold, soweit vorhanden).
- **Abnahme:** jede lädt ohne Gründe (Test in `designs.rs`), Zeilen in der Doku, Sichtprüfung je Schrift.

### SR.6 Sans 2: DM Sans, Public Sans, Lexend, Fira Sans, Geist, Rubik, Cantarell
- [ ] erledigt
- **Modell:** Opus · **Braucht:** SR.5 · **Dateien:** `fonts/{dm-sans,public-sans,lexend,fira-sans,geist,rubik,
  cantarell}/`, `crates/torroworkplace/src/designs.rs`, `docs/design-pakete.md`
- Nach den gemeinsamen Regeln.
- **Abnahme:** wie SR.5.

### SR.7 Serif und Mono: Literata, Merriweather, Lora, Newsreader, JetBrains Mono, Fira Code, Plex Mono, Source Code Pro, Geist Mono, Inconsolata
- [ ] erledigt
- **Modell:** Opus · **Braucht:** SR.6 · **Dateien:** `fonts/{literata,merriweather,lora,newsreader,jetbrains-mono,
  fira-code,plex-mono,source-code-pro,geist-mono,inconsolata}/`, `crates/torroworkplace/src/designs.rs`,
  `docs/design-pakete.md`
- Nach den gemeinsamen Regeln. Mono-Schriften: `numbers` bleibt die Textschrift (gleich breit, trotzdem prüfen).
  Danach zeigt Console seinen Vorschlag `jetbrains-mono` als vorhanden (Satz „… is suggested for Console.“).
- **Abnahme:** wie SR.5; zusätzlich `torroworkplace ui --demo --open design --design console` zeigt den Vorschlag
  ohne „isn't installed“.

### SR.8 Abschluss: Übersetzungen, L.24, Sichtprüfung
- [ ] erledigt
- **Modell:** Opus · **Braucht:** SR.7 · **Dateien:** `crates/torro-core/src/i18n/de.rs`,
  `docs/tasks/design-pakete.md`, `docs/tasks/schriften.md`
- Deutsche Übersetzungen aller neuen Texte aus SR.1–SR.4 (Gründe der Systemschriften, „Search“, „No matches“, „Torro
  fonts“, „System fonts“, „System“, „Sans“, „Serif“, „Mono“, „Other“, Fußsatz). L.24 in `design-pakete.md` als
  „gezogen nach #299 (SR.5)“ markieren.
- **Sichtprüfung** (`torro-design-review`): das Dropdown offen und mit Suche, jede mitgelieferte Schrift im Standard,
  hell und dunkel, auf macOS und Linux mit Systemschriften (ohne Hyprland als „nicht möglich“ melden).
- **Abnahme:** fmt/clippy/test, `TORRO_LANG=de cargo test --workspace`, `scripts/ci-local.sh`; `torro-api-review` ohne
  Befund; die Größe von `torroworkplace` vor und nach im Bericht.
