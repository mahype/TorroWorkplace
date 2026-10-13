# Aufgaben: Design-Pakete, Stufe 1 (L.1–L.9)

Konzept und Regeln für Pakete: [design-pakete.md](../design-pakete.md). Issue #72. Regeln für die Arbeit:
[CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`, Oberfläche: Skill `torro-design`. Wer einen
Request anfasst, lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`.

> **Entschieden 2026-10-13:** Stufe 1 wird vorgezogen, vor E3.9 (Übersicht der Inbox), damit neue Oberflächen gleich
> mit austauschbaren Werten entstehen. Logo und Akzentfarbe gehören ins Paket. Stufe 2 (Layout-Schalter) planen wir,
> wenn Stufe 1 steht.

```
L.1 ── L.2 ── L.3 ── L.4 ── L.5 ── L.6 ─┬─ L.7 Commitr
                                        ├─ L.8 Dracula
                                        └─ L.9 Ribbon
```

L.1–L.6 nacheinander: L.2 ändert jede App, L.3 den Katalog, L.6 das Format. L.7–L.9 (Issue #80) sind unabhängig
voneinander, jedes Paket wird erst durchdesignt, dann gebaut.

**Neue Abhängigkeiten** (über `[workspace.dependencies]`): `toml` (L.1), `ttf-parser` (L.1, Familie und Format
prüfen), `resvg` in der Fassung, die Iced 0.14 schon zieht (L.3, Tray-Symbol aus SVG).

### L.1 Kern: Paketformat, Standardpaket, Laden und Prüfen
- [x] erledigt
- **Modell:** Opus · **Dateien:** `crates/torro-core/src/design.rs` (neu, ggf. `design/` mit Untermodulen),
  `crates/torro-core/src/lib.rs`, `crates/torro-core/Cargo.toml`, `Cargo.toml` (Wurzel, Abhängigkeiten),
  `crates/torro-core/tests/fixtures/designs/` (Beispielpakete, gültig und kaputt)
- Modell nach [design-pakete.md](../design-pakete.md): `Design` mit `about`, bis zu drei Schriften (`text`,
  `heading`, `numbers`, je mit `regular`/`medium`/`semibold`/`bold`), acht Stufen mit Schrift, Größe, Zeilenhöhe und
  Gewicht, `space`, `radius`, Layout-Abständen (`inset`, `page_padding`, `panel_padding`, `row_padding`,
  `control_padding`, `section_gap`, `item_gap`), `size`, Farben hell und dunkel (alle Felder von `torro_gui::theme::Palette`, die nicht abgeleitet sind), Logo-Pfaden.
  Farben als `Rgba` in `torro-core` (kein Iced), Schreibweise `#RRGGBB` oder `#RRGGBBAA`.
- `Design::standard()`: **genau die heutigen Werte** aus `torro_gui::theme`, `typo` und `torro_core::theme` (Inter
  eingebettet, „T“ und Tray-Raster als Logo). Die Werte wandern hierher; `torro-gui` liest sie ab L.2 von hier.
  Für die Layout-Abstände die Werte, die Shell, Listenzeilen, Paneele, Knöpfe und Seiten heute nutzen (im Code
  nachsehen, nicht raten; im Doc-Kommentar die Fundstelle nennen).
- `designs::list(dir) -> Vec<Listed>`: Standardpaket zuerst, dann jeder Ordner unter `<data>/designs/` mit Ergebnis
  (gültig oder Liste der Gründe). `designs::load(dir, id) -> Result<Design, DesignError>`: Datei lesen, mit dem
  Standard zusammenführen, prüfen. Fehler-Enum mit einer Variante je Regel, jede mit lesbarem Text (englisch, über
  `tr()`, weil die Steuerzentrale ihn zeigt).
- **Prüfen** nach der Tabelle „Regeln“ im Konzept: Format, unbekannte Schlüssel, Kontraste (WCAG-Formel; die
  Hilfsfunktion aus den Tests von `torro_gui::theme` wandert hierher), Bereiche, Dateien (Größe, Pfade bleiben im
  Ordner, auch über `..` und Links), SVG ohne äußere Verweise und Skripte, Schriften lesbar und Familie stimmt
  (`ttf-parser`), Lizenzdatei, Logo quadratisch.
- Nichts davon führt Code aus dem Paket aus; das Lesen ist begrenzt (Größen prüfen, bevor gelesen wird).
- **Oberfläche/API/MCP:** noch keine (L.3, L.4).
- **Abnahme:** Tests: Standardpaket besteht die eigenen Regeln; nur Akzent gesetzt → Rest vom Standard; jede Regel
  mit einem kaputten Beispiel (zu wenig Kontrast, `on_accent` schlecht, Schrift 9, `row` 20, Pfad `../x`, SVG mit
  `href="https://…"`, Schrift ohne Lizenz, falsche Familie, unbekannter Schlüssel, `format = 2`); `list` zeigt
  gültige und ungültige Pakete mit Gründen. Kein echtes Home-Verzeichnis (`TORRO_DATA_HOME` auf ein Temp-Verzeichnis).

### L.2 torro-gui liest das aktive Design, alle Apps umgestellt
- [x] erledigt
- **Modell:** Opus · **Braucht:** L.1 · **Dateien:** `crates/torro-gui/src/**` (vor allem `theme.rs`, `typo.rs`,
  `lib.rs`, `shell.rs`, `style.rs`, `widgets.rs`, neu `design.rs`), `crates/torro-gui/examples/**`, alle Stellen in
  `crates/{torrocalendar,torrotasks,torroinbox}/src/**` und `crates/torroworkplace/src/ui/**`, die Tokens nutzen,
  `scripts/design-check.sh`, `.claude/skills/torro-design/SKILL.md`, `docs/tasks/design.md` (Hinweis auf die neue
  Schreibweise)
- **Schnittstelle (freigegeben 2026-10-13):** Die Konstanten werden Funktionen des aktiven Designs:
  `space::M` → `space::m()`, `radius::SM` → `radius::sm()`, `size::ROW` → `size::row()`, `typo::BODY` →
  `typo::body()`, `theme::FONT` → `theme::font()` usw.; `palette(theme)` liefert die Farben des aktiven Designs für
  den Modus des Themes (Rückgabe nach Bedarf als Wert statt `&'static`). Namen sonst gleich, damit die Umstellung
  mechanisch bleibt. `torro_gui::design::{active, set}` hält das Design je Prozess (z. B. `RwLock` hinter `LazyLock`,
  kein `unsafe`).
- **Start:** `torro_gui::settings()` lädt die Schriften des aktiven Designs; Familiennamen, die Iced als
  `&'static str` braucht, einmal je Familie festhalten (`Box::leak` ist erlaubt, begrenzt auf die geladenen Familien).
  Bis L.3 ist das aktive Design immer das Standardpaket.
- **Typografie:** Stufen bekommen Schrift und Gewicht aus dem Design (`typo::title()` nutzt z. B. die
  Überschriften-Schrift des Pakets).
- **Layout-Abstände:** neue benannte Tokens (`layout::inset()`, `layout::page_padding()`, `layout::panel_padding()`,
  `layout::row_padding()`, `layout::control_padding()`, `layout::section_gap()`, `layout::item_gap()`). Bausteine in
  `torro-gui` (Shell, `list_row`, `panel`, `popover`, `dialog`, Knöpfe, `widgets::group`) nutzen sie; in den Apps die
  Stellen umstellen, die genau diese Rollen haben (Seiten- und Paneel-Innenrand, Abstand zwischen Gruppen). Alles
  andere bleibt auf der Skala `space::*`.
- **Logo:** `shell`-Logo (Umschalter, Leiste) zeichnet `logo.mark` des Designs als SVG, sonst das „T“ auf `accent`.
- Abgeleitete Farben (`attention_text`, `error_text`, Iced-Palette) aus dem Design berechnen, nicht als Konstanten.
- `scripts/design-check.sh` und Skill `torro-design` auf die neue Schreibweise umstellen (Regeln bleiben gleich:
  keine Zahlen in Apps).
- **Oberfläche/API/MCP:** keine Änderung für Nutzer.
- **Abnahme:** fmt/clippy/test grün (auch `TORRO_LANG=de`); `scripts/design-check.sh` sauber; **mit dem
  Standardpaket gleiches Bild wie vorher**: Test, dass `Design::standard()` in `torro_gui` dieselben Werte ergibt wie
  die alten Konstanten (alte Werte einmal als Testdaten festhalten), Galerie und Demo-Starts aller Apps laufen.
  Sichtprüfung nach Möglichkeit (`torro-design-review`); ohne Hyprland ehrlich als „nicht möglich“ melden.

### L.3 Einstellung, API, MCP und Wechsel in offenen Fenstern
- [x] erledigt
- **Modell:** Opus · **Braucht:** L.2 · **Dateien:** `crates/torro-core/src/settings.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torroworkplace/src/{core,settings_service,mcp}.rs`,
  `crates/torroworkplace/src/tray/**`, `crates/torro-gui/src/{theme,design,lib}.rs`, die Stellen der Apps, an denen
  heute `Message::Appearance` ankommt (`crates/{torrocalendar,torrotasks,torroinbox}/src/app.rs`,
  `crates/torroworkplace/src/ui/**`), `Cargo.toml`; Doku: `docs/{api,settings}.md`
- `Settings::design: String` (`#[serde(default)]`, Standard `"torro"`), `SettingKey::Design`; `set_setting` lehnt
  unbekannte oder ungültige Pakete mit `error` und den Gründen ab.
- **API:** neu `settings.list_designs` · Read (Bereich `settings`): je Paket `id`, `name`, `version`, `author`,
  `description`, `standard`, `valid`, `problems` (Texte), `active`. Katalogeintrag, `docs/api.md`.
  **MCP:** `settings_list_designs`; `settings_set_setting` nennt `design` in der Beschreibung.
- **Wechsel:** Die Apps erfahren das gewählte Design auf demselben Weg wie heute `appearance` und `zoom`, laden es mit
  `torro_core::design::load`, setzen es (`torro_gui::design::set`), laden neue Schriften (`iced::font::load`) und
  zeichnen neu (Caches von Canvas leeren). Ungültig oder weg → Standardpaket, ohne Absturz. `--demo` liest das Design
  aus `--design <id>` (neu, nur mit `--demo`, wie `--appearance`).
- **Tray:** Symbol aus `logo.tray` (SVG, mit `resvg` gerastert für jede Größe), sonst das Raster in der Akzentfarbe des
  Designs; wechselt mit.
- **Abnahme:** Tests: Einstellung speichern und lesen, ungültiges Paket abgelehnt, `list_designs` mit gültigen und
  kaputten Paketen, Rechte (Read reicht zum Auflisten, Admin zum Wählen), Tray-Pixmap aus SVG und Fallback, App-Nachricht
  „Design gewechselt“ setzt das Design. `torro-api-review`.

### L.4 Steuerzentrale: Design wählen
- [x] erledigt
- **Modell:** Opus · **Braucht:** L.3 · **Dateien:** `crates/torroworkplace/src/ui/**` (Seite General),
  `crates/torroworkplace/src/ui/launch.rs` (Demo-Zustand), `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** General › Appearance: unter „Light / Dark / System“ die Gruppe „Design“ mit einer Zeile je Paket:
  Vorschau (Bild `preview.png` oder Farbmuster aus `bg`, `panel`, `accent`, `text` plus „Aa“ in der Schrift des
  Pakets), Name, Version und Autor, Häkchen beim aktiven. Ungültige Pakete ausgegraut mit „Can't be used:“ und den
  Gründen. Dazu „Open designs folder“ (legt den Ordner an, falls er fehlt) und „Reload“.
  Wählen wirkt sofort (L.3), keine Rückfrage.
- **API/MCP:** nutzt `settings.list_designs`, `settings.set_setting` (L.3).
- **Abnahme:** Demo-Zustand `--open design`, Tests der Nachrichten, Sichtprüfung Breit und Schmal, hell und dunkel.
- **Nacharbeit aus L.3** (gehört dazu; Dateien: `crates/torro-gui/src/design.rs`, `crates/torro-gui/Cargo.toml`
  (`tokio` aus dem Workspace), je eine Zeile in `crates/{torrocalendar,torrotasks,torroinbox}/src/app.rs`):
  (1) **Wechsel ohne Wartezeit:** `design::Follow::subscription()` fragt die Einstellungen alle 2 s ab und meldet nur
  eine Änderung (sonst keine Nachricht, also kein Neuzeichnen); die Apps abonnieren es statt der Abfrage im Takt.
  Appearance und Zoom kommen auf eigenem Weg und bleiben im Takt der App. Die Steuerzentrale zeichnet ein gewähltes
  Design in ihrem Fenster sofort. (2) **Schrift überall:** Jede Schrift der Bausteine kommt aus dem aktiven Design
  (`typo`, `.font(…)` an Eingaben, Editoren, Pick-Lists); was noch auf Iceds Standardschrift fiel (Schnelleingabe der
  Aufgaben-App), setzt sie jetzt ausdrücklich.

### L.5 Zweites Paket als Beispiel und Prüfstein
- [x] erledigt (2026-10-13: Plex ohne eigene Ziffernschrift – die Ziffern von IBM Plex Sans sind schon gleich breit;
  Beispielpakete stecken in `torroworkplace` (`src/designs.rs`) und kommen beim ersten Start in den Ordner, nur wenn
  er fehlt; Sichtprüfung auf macOS nur in der Fenstergröße, die das System vergibt)
- **Modell:** Opus · **Braucht:** L.4 · **Dateien:** `designs/` (neu im Repo: `designs/plex/` mit `design.toml`,
  `logo.svg`, `tray.svg`, `preview.png`, Schriften IBM Plex Sans unter OFL mit `LICENSE.txt`), `docs/design-pakete.md`
  (Abschnitt „Ein Paket bauen“ mit diesem Beispiel), `crates/torroworkplace/src/…` nur falls die Suite Beispielpakete
  beim ersten Start in den Ordner kopiert (Entscheidung in der Aufgabe, Vorschlag: ja, nur wenn der Ordner fehlt)
- Ein deutlich anderes Design: andere Schrift (IBM Plex Sans, feste Ziffern mit `pyftfeatfreeze` wie bei Inter),
  mehr Luft (`body` 14, Zeilen 44), andere Akzentfarbe (kein Rot), eigenes Logo und Tray-Symbol. Besteht alle Regeln.
- **Abnahme:** Paket lädt ohne Gründe; alle Apps mit `--demo --design plex` in allen Größen angesehen (oder ehrlich
  „nicht möglich“); `torro-design-review` mit beiden Paketen.

### L.6 Seitenleiste mit eigenen Farben
- [x] erledigt (2026-10-13: `Surface` hat dazu die Variante `Sidebar` bekommen, weil Seitenleiste und Fensterzustände
  verschiedene Flächen brauchen; Grafikregeln nur bei eigener Fläche, sonst wäre Plex ungültig geworden; Achtungs- und
  Fehlertext in der Leiste nicht geprüft, aber nach ihrer Helligkeit abgeleitet; Sichtprüfung nur breit, die
  Fenstergröße ließ sich auf macOS nicht setzen)
- **Modell:** Opus · **Braucht:** L.5 · **Dateien:** `crates/torro-core/src/design.rs`,
  `crates/torro-core/src/design/**` (Format, Zusammenführen, Prüfen, Fehler), `crates/torro-core/tests/fixtures/designs/`,
  `crates/torro-core/src/i18n/de.rs` (neue Fehlertexte), `crates/torro-gui/src/**` (vor allem `theme.rs`, `style.rs`,
  `shell.rs`, `widgets.rs`, `design.rs`), die Stellen in `crates/{torrocalendar,torrotasks,torroinbox}/src/**` und
  `crates/torroworkplace/src/ui/**`, die in der Seitenleiste selbst Farben wählen (z. B. Mini-Monat, Kalenderliste),
  `.claude/skills/torro-design/SKILL.md`; Doku: `docs/design-pakete.md` (Abschnitt steht schon)
- **Format** nach [Seitenleiste mit eigenen Farben](../design-pakete.md#seitenleiste-mit-eigenen-farben): je Modus
  optional `sidebar`, `sidebar_hover`, `sidebar_selected`, `sidebar_text`, `sidebar_text_2`, `sidebar_text_3`,
  `sidebar_line`. Fehlt ein Wert, gilt der entsprechende Wert **desselben Pakets** (`bg`, `hover`, `selected`, `text`,
  `text_2`, `text_3`, `line`), nicht der des Standards. Das Standardpaket setzt keinen davon.
- **Schnittstelle (zur Freigabe):** `Palette` bekommt die sieben Felder (in `torro-core` und `torro-gui`).
  Vorschlag für die Umsetzung, ohne die Apps anzufassen: Die Shell legt die Seitenleiste (auch Rail, Schublade, Rand
  und Titelband) in einen Iced-`themer` mit einem Theme, das `palette()` als „Seitenleiste“ erkennt und dann eine
  Palette liefert, in der `bg`/`text`/… durch die `sidebar*`-Werte ersetzt sind. So zeichnet sich alles in der Leiste
  von selbst richtig. Andere Lösung → vorher fragen.
- **Prüfen:** Regel „Seitenleiste“ aus der Tabelle im Konzept, mit Werten nach dem Zurückfallen; eine Fehlervariante
  mit Schlüssel und Verhältnis wie die übrigen Kontrastfehler.
- **Titelband:** `Surface::Window` nimmt `sidebar`, damit die Ampel auf der Leistenfarbe sitzt; Zustände auf dem Fenster
  selbst (Anmelden, Laden, Fehler) bleiben auf `bg`.
- **Oberfläche/API/MCP:** keine neuen Funktionen; `settings_list_designs` meldet die neuen Gründe wie alle anderen.
- **Abnahme:** Standardpaket und Plex sehen aus wie vorher (Test: ohne `sidebar*` ist die Seitenleisten-Palette gleich
  der normalen). Testpaket mit dunkler Leiste neben hellem Inhalt lädt; kaputte Beispiele (Text auf `sidebar` zu
  schwach, Kalenderfarbe auf `sidebar` zu schwach) werden mit Grund abgelehnt. Sichtprüfung mit einem Testpaket in
  Kalender, Aufgaben, Inbox und Steuerzentrale, breit und mittel (Rail), hell und dunkel, auch Schublade im Schmalen.

### L.7 Paket Commitr (erinnert an GitHub)
- [ ] erledigt
- **Modell:** Opus · **Braucht:** L.6 · **Dateien:** `designs/commitr/`, `crates/torroworkplace/src/designs.rs`
- Erst durchdesignen, dann bauen (wie L.5). Richtung: Mona Sans (OFL), neutrales Grau, Blau `#0969DA` als Akzent,
  feine Linien, Radius 6, eher dicht, dunkle Seitenleiste. Kein GitHub-Name und kein Octocat; „inspired by GitHub“
  in der Beschreibung, eigenes Logo.
- **Abnahme:** wie L.5.

### L.8 Paket Dracula
- [ ] erledigt
- **Modell:** Opus · **Braucht:** L.5 · **Dateien:** `designs/dracula/`, `crates/torroworkplace/src/designs.rs`
- Erst durchdesignen, dann bauen. Offizielles Schema (MIT): dunkel „Dracula“, hell „Alucard“; Violett und Pink als
  Akzent; Inter, optional JetBrains Mono (OFL) für Ziffern. Autor und Lizenz im Paket.
- **Abnahme:** wie L.5.

### L.9 Paket Ribbon (erinnert an Microsoft Office)
- [ ] erledigt
- **Modell:** Opus · **Braucht:** L.6 · **Dateien:** `designs/ribbon/`, `crates/torroworkplace/src/designs.rs`
- Erst durchdesignen, dann bauen. Richtung: Selawik (OFL; Segoe UI darf nicht mit), Office-Blau `#0F6CBD`, helle graue
  Flächen, Radius 4, ruhig; Seitenleiste ggf. in Blau. Kein Office-Name und kein Office-Logo; „inspired by Microsoft
  Office“ in der Beschreibung, eigenes Logo.
- **Abnahme:** wie L.5.
