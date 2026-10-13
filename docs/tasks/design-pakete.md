# Aufgaben: Design-Pakete, Stufe 1 (L.1–L.15)

Konzept und Regeln für Pakete: [design-pakete.md](../design-pakete.md). Issue #72. Regeln für die Arbeit:
[CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`, Oberfläche: Skill `torro-design`. Wer einen
Request anfasst, lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`.

> **Entschieden 2026-10-13:** Stufe 1 wird vorgezogen, vor E3.9 (Übersicht der Inbox), damit neue Oberflächen gleich
> mit austauschbaren Werten entstehen. Logo und Akzentfarbe gehören ins Paket. Stufe 2 (Layout-Schalter) planen wir,
> wenn Stufe 1 steht.

```
L.1 ── L.2 ── L.3 ── L.4 ── L.5 ── L.6 ─┬─ L.7 Commitr
                                        ├─ L.8 Dracula
                                        └─ L.9 Ribbon ── L.10 neue Beispiele nachlegen ── L.11 … L.15
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
- [x] erledigt (2026-10-13: Mona Sans 2.0.27 für Text, Ziffern in Inter Tabular, weil die Ziffern von Mona Sans
  verschieden breit sind und `fonttools` zum Einfrieren fehlte; Akzent ist das Grün des Primärknopfs, Fokus blau;
  dunkle Seitenleiste nur im hellen Modus; Kalenderfarben und „Merged“ etwas heller als Primer, Fehlerrot dunkel
  `#FF7B72`)
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
- [x] erledigt (2026-10-13: Selawik 1.01 ohne Medium, Hervorhebungen in SemiBold; keine blaue Seitenleiste, weil der
  blaue Akzent darauf nicht trägt – Seitenleiste in Fluent-Grau wie die Navigation der Office-Apps; Fehlerrot dunkel
  heller als Fluent)
- **Modell:** Opus · **Braucht:** L.6 · **Dateien:** `designs/ribbon/`, `crates/torroworkplace/src/designs.rs`
- Erst durchdesignen, dann bauen. Richtung: Selawik (OFL; Segoe UI darf nicht mit), Office-Blau `#0F6CBD`, helle graue
  Flächen, Radius 4, ruhig; Seitenleiste ggf. in Blau. Kein Office-Name und kein Office-Logo; „inspired by Microsoft
  Office“ in der Beschreibung, eigenes Logo.
- **Abnahme:** wie L.5.

### L.10 Neue Beispielpakete auch in bestehende Design-Ordner
- [x] erledigt (2026-10-13: `install_new` neben `install_if_missing`, das unverändert bleibt und die Liste jetzt
  mitschreibt; Dracula kommt nach dem Merge von L.8 in die Liste der Beispiele)
- **Modell:** Opus · **Braucht:** L.9 · **Dateien:** `crates/torroworkplace/src/designs.rs`,
  `docs/design-pakete.md` (Abschnitt „Ein Paket bauen“)
- Bisher kommen Beispielpakete nur in einen **neu angelegten** Design-Ordner. Wer Plex schon hat, bekommt Dracula,
  Commitr und Ribbon nie zu sehen.
- **Neu:** Der Ordner merkt sich in einer versteckten Datei `.torro-examples` (eine id je Zeile), welche Beispiele die
  Suite schon angeboten hat. Beim Start (Dienst) und bei „Open designs folder“ (Steuerzentrale) legt sie jedes
  Beispiel, das dort noch nicht steht, in den Ordner – nur wenn es dort keinen Ordner dieses Namens gibt – und trägt es
  ein. Ein gelöschtes Beispiel steht in der Liste und bleibt gelöscht; ein eigener Ordner gleichen Namens bleibt
  unberührt; vorhandene Beispiele werden nie überschrieben (auch nicht bei einer neuen Fassung).
- **Ordner ohne Liste** (von vor L.10): Als angeboten gelten Plex (das einzige Beispiel bis dahin, damit ein
  gelöschtes Plex gelöscht bleibt) und jedes Beispiel, dessen Ordner schon da ist.
- Jedes Paket wird erst in einem versteckten Ordner daneben geschrieben und dann in einem Schritt umbenannt, die
  Liste ebenso; ein abgebrochener Start hinterlässt nichts Halbes.
- **Oberfläche/API/MCP:** keine neue Funktion – das Nachlegen passiert beim Start und hinter „Open designs folder“;
  Pakete auflisten und wählen gibt es schon (`settings.list_designs`, `settings.set_setting`).
- **Abnahme:** Tests: neuer Ordner bekommt alle Beispiele und die Liste; alter Ordner mit Plex bekommt die übrigen,
  Plex bleibt unverändert; alter Ordner ohne Plex bekommt Plex nicht zurück; ein nach dem Eintragen gelöschtes
  Beispiel kommt nicht wieder; ein eigener Ordner gleichen Namens bleibt unberührt; kein versteckter Arbeitsordner
  bleibt zurück.

### L.11 Commitr mit eigenen festen Ziffern
- [x] erledigt (2026-10-13: erzeugt mit fontTools 4.66.1 und opentype-feature-freezer 1.32.2 – `pyftfeatfreeze -f
  tnum`, dann Breiten angleichen und umbenennen; was verändert ist, steht in `fonts/LICENSE.txt` des Pakets)
- **Modell:** Opus · **Braucht:** L.7 · **Dateien:** `designs/commitr/`, `crates/torroworkplace/src/designs.rs`
- Statt Inter Tabular eine Ziffernschrift aus Mona Sans: `pyftfeatfreeze -f tnum`, dann alle zehn Ziffern auf eine
  Breite und mittig (die `.tf`-Ziffern von Mona Sans sind nicht ganz gleich breit, 576–590). Name **„Commitr
  Figures“**, denn Mona Sans hat den reservierten Namen „Mona“, den eine veränderte Fassung nach OFL nicht tragen
  darf. Copyright und Lizenz in der Datei bleiben unverändert.
- **Abnahme:** wie L.5 – das Paket lädt ohne Gründe, ein Test in `designs.rs`, Sichtprüfung hell und dunkel mit
  `--demo --design <id>`.
- **Oberfläche/API/MCP:** keine neue Funktion; das Paket erscheint in General › Appearance und in
  `settings.list_designs`, gewählt wird es mit `settings.set_setting`.

### L.12 Paket Channelr (erinnert an Slack)
- [x] erledigt (2026-10-13: Lato unverändert, nur Regular und Bold – Hervorhebungen fett wie bei Slack; gewählter
  Kanal blau auf Aubergine; Akzent ist das Grün des Senden-Knopfs, eine Spur heller, damit es auch auf der Leiste
  trägt; die vier Markenfarben als Kalenderfarben, in der Helligkeit angepasst)
- **Modell:** Opus · **Braucht:** L.10 · **Dateien:** `designs/channelr/`, `crates/torroworkplace/src/designs.rs`
- Lato (OFL), Aubergine-Seitenleiste in beiden Modi (L.6), rundere Ecken, fette Titel. Kein Slack-Name, kein Logo.
- **Abnahme:** wie L.5 – das Paket lädt ohne Gründe, ein Test in `designs.rs`, Sichtprüfung hell und dunkel mit
  `--demo --design <id>`.
- **Oberfläche/API/MCP:** keine neue Funktion; das Paket erscheint in General › Appearance und in
  `settings.list_designs`, gewählt wird es mit `settings.set_setting`.

### L.13 Paket Guildr (erinnert an Discord)
- [x] erledigt (2026-10-13: Figtree als feste Schnitte aus der variablen Schrift, Ziffern in
  „Figtree Tabular“; dunkles Fehlerrot heller, im hellen Modus Kalenderfarben und Fokus dunkler)
- **Modell:** Opus · **Braucht:** L.10 · **Dateien:** `designs/guildr/`, `crates/torroworkplace/src/designs.rs`
- Figtree (OFL) statt der unfreien „gg sans“, Blurple `#5865F2` auf Dunkelgrau, Hell abgeleitet. Kein
  Discord-Name, kein Logo.
- **Abnahme:** wie L.5 – das Paket lädt ohne Gründe, ein Test in `designs.rs`, Sichtprüfung hell und dunkel mit
  `--demo --design <id>`.
- **Oberfläche/API/MCP:** keine neue Funktion; das Paket erscheint in General › Appearance und in
  `settings.list_designs`, gewählt wird es mit `settings.set_setting`.

### L.14 Paket Quattro (erinnert an Google Workspace)
- [x] erledigt (2026-10-13: Roboto Flex als feste Schnitte, alle übrigen Achsen auf ihrem Standard;
  Radien 6/10/14/16; Kalenderfarben die vier Google-Farben plus Violett, Gelb als dunkles Ocker)
- **Modell:** Opus · **Braucht:** L.10 · **Dateien:** `designs/quattro/`, `crates/torroworkplace/src/designs.rs`
- Roboto Flex (OFL), Pastellflächen nach Material 3, große Radien, die vier Farben als Kalenderfarben. Kein
  Google-Name, kein Logo.
- **Abnahme:** wie L.5 – das Paket lädt ohne Gründe, ein Test in `designs.rs`, Sichtprüfung hell und dunkel mit
  `--demo --design <id>`.
- **Oberfläche/API/MCP:** keine neue Funktion; das Paket erscheint in General › Appearance und in
  `settings.list_designs`, gewählt wird es mit `settings.set_setting`.

### L.15 Farbschemata Nord, Solarized, Catppuccin, Gruvbox
- [ ] erledigt
- **Modell:** Opus · **Braucht:** L.10 · **Dateien:** `designs/{nord,solarized,catppuccin,gruvbox}/`,
  `crates/torroworkplace/src/designs.rs`
- Je ein Paket nach dem offenen Schema (MIT), hell und dunkel, Inter wie Torro; Autor und Lizenz in `[about]`.
  Nord hell aus „Snow Storm“, Catppuccin Latte/Mocha, Solarized und Gruvbox in beiden Originalfassungen.
- **Abnahme:** wie L.5 – das Paket lädt ohne Gründe, ein Test in `designs.rs`, Sichtprüfung hell und dunkel mit
  `--demo --design <id>`.
- **Oberfläche/API/MCP:** keine neue Funktion; das Paket erscheint in General › Appearance und in
  `settings.list_designs`, gewählt wird es mit `settings.set_setting`.
