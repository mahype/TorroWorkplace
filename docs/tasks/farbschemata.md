# Aufgaben: 21 freie Farbschemata mitliefern (FS.1–FS.5)

Konzept und Format: [design-pakete.md](../design-pakete.md) (Farbschemata seit #189, „nur dunkel“ seit #291). Issue
#292. Regeln für die Arbeit: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`, Farben und
Sichtprüfung: Skill `torro-design`.

> **Entschieden 2026-10-22 (Planung #292):** Die sechs vorhandenen Schemata nach fremden MIT-Paletten (Nord,
> Solarized, Gruvbox, Dracula, Catppuccin, Everforest) bekommen ihre `LICENSE.txt` mit diesem Issue, ihre Farben
> bleiben. Die 21 neuen Schemata bringen **kein eigenes Logo** mit; die Suite zeichnet das „T“ auf ihrem Akzent.

```
#291 in main ── FS.1 Lizenzen, Liste ── FS.2 acht hell+dunkel ── FS.3 acht hell+dunkel ── FS.4 fünf nur dunkel ── FS.5 Sichtprüfung
```

**Voraussetzung:** #291 ist in `main` und in den Branch von #292 gemergt. #291 (L.31) bringt den Schlüssel
`dark_only` und schreibt `crates/torroworkplace/src/designs.rs`, dessen Tests und `docs/design-pakete.md` um; vorher
nicht beginnen. Nacheinander, weil FS.1–FS.4 dieselbe Liste in `designs.rs` und denselben Test ändern.

**Keine Änderung** an Format, Regeln, Code der Apps, API oder MCP: `settings.list_color_schemes` und
`settings.set_setting color_scheme` (MCP `settings_list_color_schemes`, `settings_set_setting`) nennen die neuen
Schemata von selbst. Namen von Farbschemata sind Daten und werden nicht übersetzt; `de.rs` bleibt unverändert.

## Gemeinsame Regeln für FS.2–FS.4

- **Ordner** `color-schemes/<id>/` mit `color-scheme.toml` und `LICENSE.txt`, ohne `logo.svg` und ohne `[logo]`.
- **`LICENSE.txt`:** die Lizenzdatei des Quell-Repositorys wörtlich (mit der Copyright-Zeile), darüber eine Zeile
  mit der Quelle (URL) und dem Abrufdatum. Tango: kein Lizenztext, sondern der Hinweis, dass die Palette des Tango
  Desktop Project gemeinfrei ist, mit Quelle. Steht im Repository eine andere Lizenz als MIT, Apache-2.0 oder
  gemeinfrei: **anhalten und fragen**.
- **Kopfkommentar** wie bei `color-schemes/nord/color-scheme.toml`: Name, Quelle, Lizenz, welche Varianten hell und
  dunkel sind (z. B. „light is Dawn“), der gewählte Akzent, wo das Schema keinen hat, und vor jedem
  `[colors.*]`-Block, welche Farben für den Kontrast verschoben wurden (alter und neuer Wert).
- **`[about]`:** `name` wie in der Tabelle, `version = "1.0.0"`, `author = "Torro, after <Schema> by <Autor> (MIT)"`,
  `description` englisch, ein Satz. **Keine Marken** in `name`, `author` und `description`: nie GitHub, VS Code,
  Atom, IBM, GNOME, KDE, Ubuntu (Primer: „after Primer (MIT)“, One: „after One Dark and One Light (MIT)“). Die
  Copyright-Zeile in `LICENSE.txt` bleibt, wie sie ist.
- **Fünf Werte je Variante** aus der Tabelle unten (aus dem Entwurf zu #291/#292, Board „Appearance · Variant B“):
  `bg`, `panel`, `text`, gedämpfter Text, `accent`. Jeden Wert gegen die Quelle prüfen; weicht die Quelle ab, gilt
  die Quelle, mit Hinweis im Bericht.
- **Abgeleitet** und vollständig eingetragen (nichts fällt stillschweigend auf Torro zurück, außer `chart`, das aus
  `calendar` folgt, und `sidebar*`, die fehlen):
  - `raised`, `hover`, `selected`, `menu`: Stufen zwischen `panel` und `text` (dunkel `selected` die hellste Fläche,
    hell die dunkelste), wie bei Nord und Gruvbox; `night` zwischen `bg` und `panel`.
  - `text_2` zwischen `text` und dem gedämpften Text; `text_3` = gedämpfter Text, angehoben, bis er ≥ 4,5:1 auf
    allen sechs Flächen hat (Kommentar).
  - `line`, `line_strong`, `line_selected` als `text` mit Deckkraft (Muster Nord), `line_selected` ≥ 3 ×
    `line_strong` und schwächer als `focus_ring`; `focus_ring` = Akzent oder Blau des Schemas.
  - `on_accent`: Weiß oder `bg`/`text` des Schemas, was ≥ 4,5:1 erreicht; reicht keins, den Akzent verschieben
    (Kommentar).
  - `ok`, `attention`, `error`, `merged` und fünf `calendar` (Blau, Grün, Orange, Magenta/Violett, Cyan) aus der
    Palette des Schemas, verschoben, wo eine Regel es verlangt (Kommentar).
  - `category` und `unread` nur setzen, wenn die Werte des Standards die Regel auf den eigenen Flächen verfehlen.
- **Prüfen:** Ein Wert ist erst fertig, wenn `design::load_color_scheme` das Schema ohne Gründe lädt. Die Ableitung
  rechnet ein Skript im Scratchpad; es wird nicht committet.
- **Einbetten:** je Schema ein `Example` in `COLOR_SCHEMES` (`crates/torroworkplace/src/designs.rs`) mit
  `color-scheme.toml` und `LICENSE.txt`, alphabetisch nach den vorhandenen; die erwarteten ids im Test
  `every_example_loads_without_reasons_in_its_folders` nachziehen, die Logo-Prüfung dort gilt nur für die zwölf
  vorhandenen Schemata. `install_new` legt neue ids in vorhandenen Datenordnern von selbst nach (nachgesehen).
- **Doku:** die Zeilen des Schemas in der Liste aus FS.1 in `docs/design-pakete.md` vervollständigen (Varianten,
  Akzent, verschobene Farben in Kürze).

## Werte aus den Paletten

Je Variante `bg` · `panel` · `text` · gedämpft · `accent`. Quelle: Repository, aus dem Werte und Lizenz kommen.

| id | Name | Quelle | hell | dunkel |
|---|---|---|---|---|
| `ayu` | Ayu | github.com/ayu-theme/ayu-colors | `#FCFCFC` `#F8F9FA` `#5C6166` `#828E9F` `#F29718` | Mirage: `#242936` `#1F2430` `#CCCAC2` `#707A8C` `#FFCC66` |
| `base16` | Base16 | github.com/tinted-theming/schemes (Default, Chris Kempson) | `#F8F8F8` `#E8E8E8` `#383838` `#B8B8B8` `#7CAFC2` | `#181818` `#282828` `#D8D8D8` `#585858` `#7CAFC2` |
| `edge` | Edge | github.com/sainnhe/edge | `#FAFAFA` `#EEF1F4` `#4B505B` `#8790A0` `#5079BE` | `#2C2E34` `#33353F` `#C5CDD9` `#758094` `#6CB6EB` |
| `flexoki` | Flexoki | github.com/kepano/flexoki | `#FFFCF0` `#F2F0E5` `#100F0F` `#6F6E69` `#205EA6` | `#100F0F` `#1C1B1A` `#CECDC3` `#878580` `#4385BE` |
| `iceberg` | Iceberg | github.com/cocopon/iceberg.vim | `#E8E9EC` `#DCDFE7` `#33374C` `#8389A3` `#2D539E` | `#161821` `#1E2132` `#C6C8D1` `#6B7089` `#84A0C6` |
| `kanagawa` | Kanagawa | github.com/rebelot/kanagawa.nvim (hell: Lotus) | `#F2ECBC` `#E7DBA0` `#545464` `#8A8980` `#4D699B` | Wave: `#1F1F28` `#2A2A37` `#DCD7BA` `#727169` `#7E9CD8` |
| `nightfox` | Nightfox | github.com/EdenEast/nightfox.nvim (hell: Dayfox) | `#F6F2EE` `#E4DCD4` `#3D2B5A` `#837A72` `#2848A9` | `#192330` `#131A24` `#CDCECF` `#738091` `#719CD6` |
| `night-owl` | Night Owl | github.com/sdras/night-owl-vscode-theme (hell: Light Owl) | `#FBFBFB` `#F0F0F0` `#403F53` `#989FB1` `#2AA298` | `#011627` `#021320` `#D6DEEB` `#637777` `#82AAFF` |
| `one` | One | github.com/atom/atom, `packages/one-dark-syntax`, `one-light-syntax` | `#FAFAFA` `#EAEAEB` `#383A42` `#A0A1A7` `#4078F2` | `#282C34` `#21252B` `#ABB2BF` `#5C6370` `#61AFEF` |
| `primer` | Primer | github.com/primer/primitives | `#FFFFFF` `#F6F8FA` `#1F2328` `#59636E` `#0969DA` | `#0D1117` `#151B23` `#F0F6FC` `#9198A1` `#4493F8` |
| `rose-pine` | Rosé Pine | github.com/rose-pine/palette (hell: Dawn) | `#FAF4ED` `#FFFAF3` `#575279` `#797593` `#907AA9` | `#191724` `#1F1D2E` `#E0DEF4` `#908CAA` `#C4A7E7` |
| `selenized` | Selenized | github.com/jan-warchol/selenized | `#FBF3DB` `#ECE3CC` `#53676D` `#909995` `#0072D4` | `#103C48` `#184956` `#ADBCBC` `#72898F` `#4695F7` |
| `slate` | Slate | github.com/radix-ui/colors (Slate, Akzent Blue 9) | `#FCFCFD` `#F9F9FB` `#1C2024` `#60646C` `#0090FF` | `#111113` `#18191B` `#EDEEF0` `#B0B4BA` `#0090FF` |
| `tango` | Tango | Tango Desktop Project, Palette (gemeinfrei) | `#EEEEEC` `#D3D7CF` `#2E3436` `#888A85` `#3465A4` | `#2E3436` `#555753` `#EEEEEC` `#BABDB6` `#729FCF` |
| `tokyo-night` | Tokyo Night | github.com/enkia/tokyo-night-vscode-theme (hell: Light) | `#E6E7ED` `#D6D8DF` `#343B59` `#707280` `#2959AA` | `#1A1B26` `#16161E` `#C0CAF5` `#565F89` `#7AA2F7` |
| `tomorrow` | Tomorrow | github.com/chriskempson/tomorrow-theme | `#FFFFFF` `#EFEFEF` `#4D4D4C` `#8E908C` `#4271AE` | Night: `#1D1F21` `#282A2E` `#C5C8C6` `#969896` `#81A2BE` |
| `cobalt2` | Cobalt2 | github.com/wesbos/cobalt2-vscode | – | `#193549` `#15232D` `#FFFFFF` `#AAAAAA` `#FFC600` |
| `palenight` | Palenight | github.com/whizkydee/vscode-palenight-theme | – | `#292D3E` `#31364A` `#BFC7D5` `#697098` `#C792EA` |
| `poimandres` | Poimandres | github.com/drcmda/poimandres-theme | – | `#1B1E28` `#232733` `#A6ACCD` `#767C9D` `#5DE4C7` |
| `sonokai` | Sonokai | github.com/sainnhe/sonokai | – | `#2C2E34` `#33353F` `#E2E2E3` `#7F8490` `#76CCE0` |
| `synthwave-84` | Synthwave ’84 | github.com/robb0wen/synthwave-vscode | – | `#262335` `#241B2F` `#FFFFFF` `#848BBD` `#FF7EDB` |

**Akzent ohne Vorbild:** Flexoki Blau `#205EA6` hell (Weiß darauf 6,5:1) und `#4385BE` dunkel (`#100F0F` darauf
4,9:1), im Kopfkommentar vermerkt, dazu in `description` der Link „stephango.com/flexoki“, um den der Autor bittet.
Gruvbox bleibt, wie es ist: Der im Issue vorgeschlagene helle Akzent `#B57614` trägt weder Weiß (3,8:1) noch dunkle
Schrift (3,9:1); der heutige Akzent `#AF3A03` (6,1:1) bleibt, sein Kopfkommentar nennt die Wahl schon.

### FS.1 Lizenzen der vorhandenen Schemata, Liste in der Doku
- [ ] erledigt
- **Modell:** Opus · **Braucht:** #291 in `main` · **Dateien:** `color-schemes/{nord,solarized,gruvbox,dracula,catppuccin,everforest}/LICENSE.txt` (neu), `crates/torroworkplace/src/designs.rs`, `docs/design-pakete.md`
- `LICENSE.txt` für die sechs nach den gemeinsamen Regeln (Quellen: arcticicestudio/nord, altercation/solarized,
  morhetz/gruvbox, dracula/dracula-theme, catppuccin/catppuccin, sainnhe/everforest). Die eigenen Schemata Torro,
  Channelr, Commitr, Guildr, Plex, Quattro und Ribbon bekommen keine. Farben und `[about]` bleiben unverändert.
- In `COLOR_SCHEMES` die sechs um `LICENSE.txt` ergänzen.
- `docs/design-pakete.md`: neuer Abschnitt „Mitgelieferte Farbschemata“ nach „Mitgelieferte Designs“ mit einer
  Tabelle aller mitgelieferten Schemata (Name, id, Gruppe Torro / Light and dark / Dark only, Quelle, Lizenz,
  Anmerkung) – die 13 vorhandenen vollständig, die 21 neuen als Zeilen, die FS.2–FS.4 vervollständigen. Dazu die
  Regeln aus „Hintergrund“ von #292 in drei Sätzen: nur MIT, Apache-2.0 oder gemeinfrei, mit `LICENSE.txt`; neutrale
  Namen statt Marken; nicht mitgeliefert und warum (Copyleft: Adwaita, Breeze, Yaru, Modus, Zenburn; ohne klare
  Lizenz: Monokai, Material Theme, Dracula PRO). Im Abschnitt „Ein Paket bauen“ unter „Farbschema“ ergänzen, dass
  ein Schema nach fremder Palette deren Lizenz als `LICENSE.txt` mitbringt.
- **Abnahme:** `the_embedded_files_are_the_whole_example` und `every_example_loads_without_reasons_in_its_folders`
  grün; fmt/clippy/test, auch `TORRO_LANG=de`.

### FS.2 Acht Schemata hell und dunkel: Ayu bis Night Owl
- [ ] erledigt
- **Modell:** Opus · **Braucht:** FS.1 · **Dateien:** `color-schemes/{ayu,base16,edge,flexoki,iceberg,kanagawa,nightfox,night-owl}/{color-scheme.toml,LICENSE.txt}` (neu), `crates/torroworkplace/src/designs.rs`, `docs/design-pakete.md`
- Nach den gemeinsamen Regeln und der Tabelle; Flexoki mit Link und gewähltem Akzent.
- **Abnahme:** die acht laden ohne Gründe (Test); Kommentar bei jeder verschobenen Farbe; fmt/clippy/test, auch
  `TORRO_LANG=de`.

### FS.3 Acht Schemata hell und dunkel: One bis Tomorrow
- [ ] erledigt
- **Modell:** Opus · **Braucht:** FS.2 · **Dateien:** `color-schemes/{one,primer,rose-pine,selenized,slate,tango,tokyo-night,tomorrow}/{color-scheme.toml,LICENSE.txt}` (neu), `crates/torroworkplace/src/designs.rs`, `docs/design-pakete.md`
- Nach den gemeinsamen Regeln und der Tabelle; Primer und One ohne Marken in `[about]`; Tango mit dem Hinweis auf die
  gemeinfreie Palette statt Lizenztext.
- **Abnahme:** wie FS.2, für diese acht.

### FS.4 Fünf Schemata nur dunkel
- [ ] erledigt
- **Modell:** Opus · **Braucht:** FS.3 · **Dateien:** `color-schemes/{cobalt2,palenight,poimandres,sonokai,synthwave-84}/{color-scheme.toml,LICENSE.txt}` (neu), `crates/torroworkplace/src/designs.rs`, `docs/design-pakete.md`
- Nach den gemeinsamen Regeln, mit `dark_only = true` (#291, L.31) und nur `[colors.dark]`.
- Test: die fünf haben `ColorScheme::dark_only`, alle anderen mitgelieferten nicht.
- **Abnahme:** wie FS.2, für diese fünf, dazu der Test zu `dark_only`.

### FS.5 Sichtprüfung und Abschluss
- [ ] erledigt
- **Modell:** Opus · **Braucht:** FS.4 · **Dateien:** `docs/tasks/farbschemata.md`, Farbwerte in
  `color-schemes/<id>/color-scheme.toml` nur, falls die Sichtprüfung einen Befund ergibt
- Sichtprüfung (`torro-design-review`) je neues Schema, hell und dunkel (nur dunkel: einmal): `TORRO_DATA_HOME` auf
  ein Temp-Verzeichnis, `color-schemes/` des Repositorys dorthin kopieren, dann `torroinbox --demo --color-scheme <id>
  --appearance light|dark` und `torrocalendar --demo --color-scheme <id> --appearance light|dark`, je in Breit.
  Farbschemata ändern kein Layout, darum reicht eine Größenklasse. Zu sehen: Text lesbar, Auswahl und Hover
  unterscheidbar, Akzent und Kalenderfarben passen zum Vorbild, „T“-Logo auf dem Akzent lesbar. Dazu einmal
  Steuerzentrale › Appearance (`torroworkplace ui --demo --section appearance`): das Dropdown zeigt alle Schemata in
  „Torro“, „Light and dark“ und „Dark only“ mit vier Streifen.
- Ohne Hyprland ist `shoot.sh` nicht nutzbar: Sichtprüfung als „nicht möglich“ melden und die Befehle für die
  Prüfung von Hand im Bericht nennen.
- **Abnahme:** Befunde behoben oder im Bericht; `scripts/ci-local.sh` grün.
