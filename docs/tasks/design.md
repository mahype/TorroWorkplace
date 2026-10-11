# Aufgaben: modernes Design für alle Oberflächen

Stand: 2026-10-11. Grundlage ist die Design-Prüfung aller Oberflächen (Kalender, Aufgaben,
Steuerzentrale) durch den Agenten `torro-design-review`. Regeln und Werte: Skill
`.claude/skills/torro-design` (überarbeitet mit diesem Plan). Regeln für die Arbeit:
[CLAUDE.md](../../CLAUDE.md).

## Befund in einem Absatz

Es gibt kein gemeinsames Fundament. Inter ist nicht eingebunden (es rendert Liberation Sans ohne
Semibold), im Code stehen 9 Schriftgrößen und alles ohne `.size()` fällt auf Iced-Standard 16.
Knöpfe, Eingaben und Häkchen sind Iced-Standard (graue Klötze, rote Häkchen), Rot steht
gleichzeitig für Auswahl, Primäraktion, Heute und Jetzt. Abstände: 10 Werte für `spacing`,
16 Kombinationen für `padding`. Kleine Kacheln werden gequetscht statt umgebaut, große gestreckt.
Die Steuerzentrale hat sieben verschiedene Kartenstile.

## Ziel und Vorbilder

Ruhig, dicht genug für Arbeit, aus einem Guss – auf dem Niveau von **Linear, Things 3,
Fantastical, Notion Calendar, Amie, Raycast** und der macOS-15-Systemeinstellungen. Die Werte im
Skill `torro-design` sind daraus abgeleitet (Inter 14 als Grundgröße, Gewicht 500 statt Fett,
Steuerelemente 28–36 hoch, Radien 4/6/10/14, neutrale Auswahl, getönte Termine mit Farbbalken).

## Reihenfolge

1. **Fundament** F.1 → F.2 → (F.3, F.4, F.8, K.18 parallel) → F.5, dazu K.18 (Größenklassen, in
   [kalender-aufgaben.md](kalender-aufgaben.md)) und F.6. Danach dürfen Apps keine eigenen
   Zahlen und keine Iced-Standardstile mehr verwenden.
2. **Q.1** Sichtprüfung für Zustände, die nur per Klick erreichbar sind.
3. **Apps**, untereinander parallel: Kalender C.*, Aufgaben T.*, Steuerzentrale W.*. K.19 und
   K.20 (Mini, Leiste, Schmal) laufen nach C.1 bzw. T.1 und nutzen das neue Fundament.
4. Nach jeder App-Aufgabe: `torro-design-review` in allen sechs Größen. Abgenommen ist eine
   Aufgabe erst, wenn alle vier Ziele des Skills mindestens „erfüllt“ sind.

Vorbild ist **Linear** (entschieden am 2026-10-11); die verbindliche Referenz sind die Entwürfe in
[docs/prototypes/linear/](../prototypes/linear/) (HTML + Screenshots in drei Größen, hell und dunkel).
Jede App-Aufgabe vergleicht ihr Ergebnis mit dem passenden Screenshot.

Die Aufgaben ändern nur das Aussehen und die Bedienung, keine Funktionen – außer F.8
(Erscheinungsbild), die Oberfläche, API und MCP nennt. Sonst sind API und MCP (`docs/api.md`) nicht betroffen.

## Aufgaben

### Fundament

### F.1 Schrift: Inter und Schriftstufen
- [x] erledigt
- **Modell:** Opus · **Ersetzt:** K.17
- **Dateien:** `crates/torro-gui/assets/fonts/` (neu), `crates/torro-gui/src/{lib,theme,connect}.rs`,
  `crates/torro-gui/src/typo.rs` (neu), `crates/torrocalendar/src/main.rs`,
  `crates/torrotasks/src/main.rs`, `crates/torroworkplace/src/ui/mod.rs`,
  `crates/torroworkplace/src/ui/wizard.rs`
- Inter 4.x (SIL OFL 1.1, github.com/rsms/inter): statische TTF **Regular, Medium, SemiBold**
  plus `OFL.txt`. Dazu eine Instanz mit fest eingebauten tabellarischen Ziffern („Inter Tabular“,
  Regular und Medium; erzeugt mit `pyftfeatfreeze -f tnum -S -U Tabular`), weil Iced 0.14
  OpenType-Features nicht schalten kann. Das Erzeugen als Befehl im Doc-Kommentar festhalten.
- `torro_gui::fonts()` (eingebettete Dateien), `theme::FONT`, `FONT_MEDIUM`, `FONT_SEMIBOLD`,
  `FONT_TABULAR`, `FONT_TABULAR_MEDIUM`.
- `typo.rs`: die Stufen aus dem Skill als Konstanten (`CAPTION`, `META`, `BODY`, `BODY_STRONG`,
  `TITLE`, `HEADLINE`, `DISPLAY`) mit Größe, absoluter Zeilenhöhe und Schnitt, und **eine**
  Hilfsfunktion, die einen `text` in einer Stufe baut (Signatur entwirft diese Aufgabe; Ziel:
  Apps schreiben nie mehr `.size(…)` von Hand).
- Alle drei Programme (Kalender, Aufgaben, Steuerzentrale) laden die Schriften, setzen
  `default_font(theme::FONT)` und Standardgröße 14. `Font::DEFAULT` mit Gewicht in `wizard.rs`
  und `connect.rs` ersetzen.
- **Abnahme:** Test, dass jede Datei eine gültige TTF ist; Sichtprüfung zeigt Inter (z. B. das
  einstöckige „a“ und das flache „t“). Im Bericht die Inter-Version.

### F.2 Farben, Abstände, Radien, Maße als Tokens – hell und dunkel
- [x] erledigt
- **Modell:** Opus · **Braucht:** F.1
- **Dateien:** `crates/torro-gui/src/theme.rs`, `crates/torro-gui/src/typo.rs`, `crates/torro-gui/src/lib.rs`
- Werte genau aus dem Skill (Abschnitte 1–3), Vorbild Linear, Referenz `docs/prototypes/linear/torro.css`.
- **Hell und dunkel zur Laufzeit:** Farben sind keine festen Konstanten mehr, die Views direkt
  benutzen. `theme.rs` bietet eine Palette je Modus (`Palette::dark()`, `Palette::light()`) mit den
  Tokens `BG`, `PANEL`, `RAISED`, `HOVER`, `SELECTED`, `MENU`, `TEXT`, `TEXT_2`, `TEXT_3`, `LINE`,
  `LINE_STRONG`, `FOCUS_RING`, `ACCENT`, Kalenderfarben samt Tönung; dazu `Appearance { System,
  Light, Dark }` und die Iced-`Theme`s „Torro Dark“/„Torro Light“. Views kommen an die Farben nur
  über `&Theme` (Style-Closures) bzw. über eine Funktion `theme::palette(&Theme) -> &Palette`. Die
  Schnittstelle so entwerfen, dass F.3/F.5 und die App-Aufgaben sie ohne Kontext nutzen können.
- `space::{XS, S, M, L, XL, XXL}`, `radius::{XS, SM, MD, LG}`, `size::{CONTROL_SM, CONTROL,
  CONTROL_LG, ROW, ROW_SIDEBAR, ROW_GROUP, HEADER, SIDEBAR, PROPS, LIST, LIST_MEDIUM, HOUR, AXIS,
  AXIS_NARROW, INSET, CONTENT_MAX, FORM_MAX}`.
- `typo.rs` auf Grundgröße 13 umstellen (Stufen aus dem Skill inkl. neuer Stufe `MICRO`);
  `settings()` setzt 13 als Standardgröße.
- Die alten Konstanten (`BACKGROUND`, `SURFACE`, `MUTED` …) bleiben vorerst als
  `#[deprecated]`-Aliase auf die dunklen Werte, damit die Apps weiter bauen; die App-Aufgaben
  stellen um, W.5 entfernt sie.
- Kontraste als Test: jede Textfarbe gegen jede Fläche, in beiden Modi (Werte im Skill, Abschnitt 4).
- Geänderte Farben im Repo `torro-design` nachziehen: im Bericht als Rückfrage nennen.

### F.3 Stile für alle Iced-Widgets
- [x] erledigt
- **Modell:** Opus · **Braucht:** F.2
- **Dateien:** `crates/torro-gui/src/style.rs` (neu), `crates/torro-gui/src/lib.rs`
- Style-Funktionen nach Skill Abschnitt 5, beide Modi: `button` (`primary`, `secondary`, `ghost`,
  `icon`, `boxed`, `danger`, `segment(active)`, `sidebar_item(active)`, `list_row(selected)`,
  `menu_item(highlighted)`), `text_input`, `text_editor`, `pick_list` samt Menü, `checkbox`
  (Kalenderfarbe, quadratisch) und `task_check` (rund, Listenfarbe), `toggler`, `radio`,
  `scrollable` (schmale Leiste), `container` (`panel` = eingesetzte Inhaltsfläche, `sidebar`,
  `header`, `group`, `menu`, `popover`, `chip`, `kbd`). Hover, gedrückt, Fokus, deaktiviert.
- **Abnahme:** Unit-Tests für die Zustandslogik (Auswahl nie Rot, deaktiviert ohne Hover, beide
  Modi); Galerie `cargo run -p torro-gui --example gallery [-- --light]`, die jeden Stil in jedem
  Zustand zeigt, Screenshot hell und dunkel im Bericht.

### F.4 Icons
- [x] erledigt
- **Modell:** Haiku · **Braucht:** F.2
- **Dateien:** `crates/torro-gui/assets/icons/` (neu), `crates/torro-gui/src/icon.rs` (neu),
  `crates/torro-gui/src/lib.rs`, `Cargo.toml` (Iced-Feature `svg`)
- Lucide (ISC-Lizenz) als SVG, nur benötigte: chevron-left/right/down, plus, refresh-cw, check,
  x, more-horizontal, calendar, list-todo, inbox, sun, calendar-clock, settings, user, layout-grid,
  app-window, search, flag, circle-alert, sidebar. `Icon`-Enum, `icon(Icon, size) -> Element` in
  Textfarbe einfärbbar. Lizenz beilegen.
- **Abnahme:** Test, dass jedes Icon geladen werden kann; keine Unicode-Glyphen mehr als Icons
  (gilt ab den App-Aufgaben).

### F.5 Gemeinsame Bausteine (Linear-Fenster)
- [x] erledigt
- **Modell:** Opus · **Braucht:** F.3, F.4, K.18
- **Dateien:** `crates/torro-gui/src/widgets.rs`, `crates/torro-gui/src/shell.rs` (neu), `crates/torro-gui/src/lib.rs`
- `shell.rs`: das Fenster aus Skill Abschnitt 0 – `shell(size_class, sidebar, header, body, props)`
  setzt Seitenleiste auf `BG`, eingesetzte Inhaltsfläche (Breit) bzw. bündigen Inhalt (Mittel,
  Schmal), Eigenschaften-Spalte nur in Breit. Dazu `app_switcher(current)` (Logo, App-Name ▾,
  Suche, „Neu“; das Menü listet die Apps der Suite und gibt eine Nachricht `OpenApp(binary)` zurück).
- Bausteine: `header_bar(leading, title, trailing)`, `icon_button`, `segmented(options, active)`,
  `sidebar_item(icon, label, count, kbd, active)`, `sidebar_group(label)`, `list_row`, `group_head`,
  `menu(sections)` + `menu_button` (Iced-Overlay, schließt bei Klick daneben und Escape),
  `popover`, `chip`, `pill`, `kbd`, `prop_row(icon, value)`, `section_title`, `group(title, rows,
  footer)`, `content(max_width, …)`, `loading()`, `error_state(what, retry)`, `empty_state`,
  `status_dot`, `avatar_stack`.
- **Abnahme:** Galerie aus F.3 um die Bausteine und ein Beispiel-Fenster in Breit, Mittel und Schmal
  erweitern; Screenshots hell und dunkel; Vergleich mit `docs/prototypes/linear/*.png`.

### F.6 Prüfung im Build
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** F.5
- **Dateien:** `scripts/design-check.sh` (neu), `.github/workflows/ci.yml`
- Schlägt fehl, wenn in `crates/{torrocalendar,torrotasks}/src` oder
  `crates/torroworkplace/src/ui` vorkommt: Zahlen in `.size(`, `.spacing(`, `.padding(`,
  `.width(`/`.height(` (außer `Length::…` und Tokens), `Color::from_rgb`, `button::primary`/
  `secondary`/`text`, `Font::DEFAULT`. Ausnahmen nur mit Kommentar `// design-check: allow <Grund>`.
  Erst als Warnung, nach C/T/W als Fehler.

### F.8 Erscheinungsbild hell/dunkel einstellen
- [x] erledigt
- **Modell:** Haiku · **Braucht:** F.2
- **Dateien:** `crates/torro-core/src/settings.rs`, `crates/torro-core/src/theme.rs`, `crates/torro-gui/src/theme.rs`, `crates/torroworkplace/src/ui/general.rs`, `crates/torrocalendar/src/{app,main}.rs`, `crates/torrotasks/src/{app,main}.rs`, `crates/torroworkplace/src/ui/mod.rs`
- `Appearance` (mit serde) zieht nach `torro_core::theme`, `torro_gui::theme` exportiert es weiter (`pub use`); `for_appearance` bleibt. „System“ über `iced::system::theme()` und `iced::system::theme_changes()`.
- **Oberfläche:** Steuerzentrale › General › „Appearance: System / Light / Dark“.
  **API:** Feld `appearance` in `Settings` (über `UpdateSettings`/`settings.set`).
  **MCP:** `settings_set` (Admin-Bereich, siehe `docs/api.md`).
- Alle drei Programme lesen die Einstellung beim Start und folgen Änderungen (Neuladen der Settings
  im vorhandenen Takt); „System“ folgt `prefers-color-scheme` bzw. dem Portal, sonst dunkel.

### F.7 Fenster deckend unter Omarchy
- [x] erledigt
- **Modell:** Haiku
- **Dateien:** `integrations/hyprland/` (Fensterregel-Datei, neu), `README.md` (ein Absatz)
- Omarchy macht alle Fenster leicht durchsichtig; auf dem sehr dunklen Hintergrund scheint
  fremder Text durch. Fensterregel „volle Deckkraft“ für die Klassen `torro-*`, in der Syntax der
  installierten Hyprland-Version (0.56 nutzt Lua-Konfiguration, siehe `autostart.lua`).

### Sichtprüfung

### Q.1 Zustände per Startparameter
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** –
- **Dateien:** `crates/torrocalendar/src/{main,app}.rs`, `crates/torrotasks/src/{main,app}.rs`,
  `crates/torroworkplace/src/ui/mod.rs`, `crates/torroworkplace/src/main.rs`
- `--demo --open <zustand>` öffnet direkt: Kalender `editor`, `editor-new`, `day`, `month`,
  `agenda`, `error`, `empty`, `loading`; Aufgaben `detail`, `error`, `empty`, `loading`.
- Steuerzentrale bekommt `--demo` (Beispielkonten, -vorlagen und -einstellungen im Speicher,
  kein Dienst) und `--open wizard`, `--open account-detail`.
- So kann `torro-design-review` jeden Zustand per Skript fotografieren, ohne zu klicken.

### Kalender

### C.1 Kopfleiste und Akzent
- [x] erledigt
- **Modell:** Haiku · **Braucht:** F.5
- **Dateien:** `crates/torrocalendar/src/app.rs`
- `header_bar`: links „Today“ (secondary, `CONTROL_SM`), ‹ › als `icon_button`, Titel
  `HEADLINE` („October 2026“, Woche als `TEXT_2` dahinter). Rechts `segmented` Day/Week/Month/
  Agenda (aktiv neutral), „+“ als einziger Primärknopf, Aktualisieren als Ghost-Icon.
- Seitenleiste mit `sidebar` (Breite `SIDEBAR`), Kalender als Zeilen mit Häkchen in
  Kalenderfarbe, Überschrift `section_title`.
- Laden/Fehler über `loading()`/`error_state` mit „Retry“.

### C.2 Woche und Tag: Termine, Raster, Heute, Jetzt
- [x] erledigt
- **Modell:** Opus · **Braucht:** F.5
- **Dateien:** `crates/torrocalendar/src/views/week.rs`, `crates/torrocalendar/src/views/week/layout.rs`
- Terminblöcke: Kalenderfarbe 18 % als Fläche, 3-px-Balken links in voller Farbe, Titel
  `META`/500 in `TEXT`, Zeit `META` in `TEXT_2`, Radius `SM`, vergangene 55 % Deckkraft. Kein Rahmen.
- Raster nur Innenlinien in `LINE`, Halbstunden weglassen; Zeitachse 48 breit, `META` tabellarisch.
- Heute im Spaltenkopf als roter Kreis mit weißer Zahl; Jetzt-Linie in `NOW` (1 px + 6-px-Punkt).
- „all day“ in `CAPTION`; ganztägige Termine im Stil der Blöcke.

### C.3 Monat
- [x] erledigt
- **Modell:** Haiku · **Braucht:** C.2
- **Dateien:** `crates/torrocalendar/src/views/month.rs`
- Nur Innenlinien, keine Zellrahmen; Tageszahl 12/500 mit Rand 8, heute als Kreis 22;
  Nachbarmonate in `TEXT_3` ohne eigene Fläche (kein `FAINT` für Text); Termine als Zeilen 20 mit
  Farbpunkt 6, „+3 more“ in `TEXT_2`.

### C.4 Agenda
- [x] erledigt
- **Modell:** Haiku · **Braucht:** F.5
- **Dateien:** `crates/torrocalendar/src/views/agenda.rs`
- Zwei Spalten: Zeit 56 breit tabellarisch in `TEXT_2` | Titel `BODY_STRONG`, Ort `META` darunter.
  Hover `list_row`. Tageskopf `CAPTION` mit Datum rechts in `TEXT_3`. Inhalt `content(CONTENT_MAX)`.

### C.5 Editor
- [x] erledigt
- **Modell:** Haiku · **Braucht:** F.5
- **Dateien:** `crates/torrocalendar/src/editor.rs`
- Titel als großes randloses Feld (`TITLE`), Felder `CONTROL` hoch mit `style::text_input`,
  Label→Feld 4, Gruppe→Gruppe 16, Datum und Zeit in einer Zeile, Dialog `R_XL`.
- Fehler erst nach dem ersten Speicherversuch oder beim Verlassen des Feldes zeigen; Speichern
  bleibt aktiv und setzt den Fokus auf das erste fehlerhafte Feld. „Delete“ als `danger` links.

(Mini, Leiste und Schmal: K.19 in [kalender-aufgaben.md](kalender-aufgaben.md), nach C.1.)

### Aufgaben

### T.1 Seitenleiste und Listenzeile
- [x] erledigt
- **Modell:** Haiku · **Braucht:** F.5
- **Dateien:** `crates/torrotasks/src/app.rs`, `crates/torrotasks/src/list.rs`
- Seitenleiste mit `sidebar`: Icons für Today/Planned/All, Zähler rechts in `META`
  tabellarisch, „Lists“ als `section_title`, Auswahl neutral (nicht mehr Rot).
- Zeile 32 hoch: runder `task_check` 18, Titel `BODY` einzeilig mit Auslassung (`Fill`), rechts
  Fälligkeit `META` (überfällig in Attention-Orange **mit** Text), Priorität als `flag`-Icon statt
  „!!“. Hover/Auswahl über `list_row`. Unter 480 Breite rutschen Fälligkeit und Priorität in eine
  zweite Zeile. Liste in `content(CONTENT_MAX)`.

### T.2 Kopf und Schnelleingabe
- [x] erledigt
- **Modell:** Haiku · **Braucht:** T.1
- **Dateien:** `crates/torrotasks/src/app.rs`
- `header_bar`: Titel `HEADLINE` mit Zahl offener in `TEXT_2`; „Show completed“ als Ghost-Icon
  mit Tooltip bzw. im „…“-Menü; Aktualisieren als Ghost-Icon.
- Schnelleingabe als Zeile im Listenstil („+ New task“, `CONTROL` hoch, Fläche `L1`, Radius `MD`)
  statt einer vollen Eingabebox. Laden/Fehler über `loading()`/`error_state`, Leer über `empty_state`.

### T.3 Tastatur
- [x] erledigt
- **Modell:** Haiku · **Braucht:** T.1
- **Dateien:** `crates/torrotasks/src/app.rs`
- ↑/↓ Auswahl, Enter öffnet das Detail, Leertaste hakt ab, N fokussiert die Schnelleingabe,
  Escape schließt das Detail. Fokus sichtbar (`FOCUS_RING`). Muster wie die Kurzbefehle im Kalender.

### T.4 Detail
- [x] erledigt
- **Modell:** Haiku · **Braucht:** T.1
- **Dateien:** `crates/torrotasks/src/detail.rs`, `crates/torrotasks/src/app.rs`
- Breit: 360–400 neben der Liste mit Linie links; Mittel: Overlay; Schmal: ersetzt die Liste
  (mit K.20 abstimmen). Titel `TITLE` randlos, Labels in Satzschreibung `CAPTION` statt Versalien,
  Label→Feld 4, Gruppe→Gruppe 16; Fälligkeit als Chips (Today, Tomorrow, Next week, Pick…).
  Fehler wie C.5 erst nach Speicherversuch.

(Mini, Leiste und Schmal: K.20 in [kalender-aufgaben.md](kalender-aufgaben.md), nach T.1.)

### Steuerzentrale

### W.1 Seitengerüst
- [x] erledigt
- **Modell:** Haiku · **Braucht:** F.5
- **Dateien:** `crates/torroworkplace/src/ui/mod.rs`
- Seitenleiste über `sidebar` (Breite `SIDEBAR`, mit Icons). Jede Seite: `header_bar` mit Titel
  `HEADLINE` und Seitenaktion rechts (z. B. „Add account“), Inhalt `content(CONTENT_MAX)`, Rand
  24 bzw. 16 (Schmal). Zurück-Knopf als `icon_button`. Startbreite aus der tatsächlichen
  Fenstergröße statt fest 1100.

### W.2 Einheitliche Gruppen auf allen Seiten
- [x] erledigt
- **Modell:** Haiku · **Braucht:** W.1
- **Dateien:** `crates/torroworkplace/src/ui/{overview,accounts,workspaces,apps,general}.rs`
- Die sieben eigenen Karten durch `group` ersetzen (Fläche `L1`, Radius `LG`, Zeilen 44–56 mit
  Trennlinien, Chevron-Icon bei Zeilen, die eine Seite öffnen). Konto-Namen mit Auslassung.

### W.3 Overview
- [x] erledigt
- **Modell:** Haiku · **Braucht:** W.2
- **Dateien:** `crates/torroworkplace/src/ui/overview.rs`
- Kacheln flexibel (min. 160, `FillPortion`) statt fest 200; Status als `status_dot` + Wort in
  `TEXT`, Farbe nur im Punkt.

### W.4 Workspaces und Apps: eine Aktion je Zeile
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** W.2
- **Dateien:** `crates/torroworkplace/src/ui/{workspaces,apps}.rs`
- Je Zeile eine sichtbare Aktion („Load“ bzw. „Open“, secondary `CONTROL_SM`), Umbenennen und
  Löschen im „…“-Menü; Fensterklasse in `META`/`TEXT_3` in der Zeile statt eigener Zeile.

### W.5 Assistent und Abschluss
- [ ] erledigt
- **Modell:** Opus · **Braucht:** W.2, C.1–C.5, T.1–T.4
- **Dateien:** `crates/torroworkplace/src/ui/wizard.rs`, `crates/torro-gui/src/theme.rs`,
  `scripts/design-check.sh`, `crates/torro-core/src/i18n/de.rs`
- Assistent: Anbieter als `group`-Zeilen, „Coming soon“ als `badge`, Dialog `R_XL`, Kopf `TITLE`.
- Abschluss: veraltete Farbnamen aus F.2 entfernen, `design-check.sh` von Warnung auf Fehler,
  Übersetzungen eintragen, `torro-design-review` über alle drei Programme in allen sechs Größen.
