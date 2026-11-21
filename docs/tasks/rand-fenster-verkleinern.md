# Aufgaben: Rand-Verkleinerung im Tiling-Modus (RV)

**Status: Konzept, Issue #951.** Es gibt noch keinen Plan und keinen Code; Aufgaben entstehen erst mit dem Plan zum
Issue.

Rahmen: [modes.md](../modes.md) (Zwei Modi, hier der Tiling-Modus), [research.md](../research.md) (Window Manager je
System), `crates/torro-wm` (Trait `WindowManager` mit Hyprland, AeroSpace, GlazeWM), `torro_gui::layout`
(Größenklassen). Regeln: [CLAUDE.md](../../CLAUDE.md). Wer einen Request anfasst, lädt `torro-api`; nach jeder
UI-Änderung prüft `torro-design-review`.

## Idee

Auf großen Bildschirmen (breit, Ultrawide) ist es mühsam, an den Rändern zu arbeiten. Im Tiling-Modus soll es eine
**Option „Rand-Verkleinerung“** geben: Zieht man ein Fenster nach außen, wird es kleiner. Der Fokus liegt in der
Mitte, die Ränder sind Peripherie.

**Svens Wunsch:**
- einstellbar für **links**, **rechts** oder **beide Seiten**;
- die Fenster werden am Rand **klein** dargestellt;
- Torro-Fenster nutzen dabei ihre **Responsive-Größenklassen** aus `torro_gui::layout` und zeigen am Rand die
  kompakte Darstellung.

Größenklassen (`crates/torro-gui/src/layout.rs`, logische Pixel):

| Klasse | Bedingung | Konstante |
|---|---|---|
| Strip | Höhe < 280 und Breite ≥ 600 | `STRIP_MAX_HEIGHT`, `NARROW_MAX_WIDTH` |
| Mini | Breite < 360 oder Höhe < 280 | `MINI_MAX_WIDTH`, `STRIP_MAX_HEIGHT` |
| Narrow | Breite < 600 | `NARROW_MAX_WIDTH` |
| Medium | Breite < 1024 | `MEDIUM_MAX_WIDTH` |
| Wide | sonst | – |

## Bezug zum Video

Scott Jenson, „Are we really going to use the same Desktop UX forever?“ (KDE Community, kde2026, 40:55):
https://youtu.be/V7AfAcQwLW0

Aus dem (automatischen, englischen) Transkript:
- Auf einem sehr breiten Bildschirm sei es „very difficult to work on the sides“; er eigne sich für das periphere
  Sehen, nicht zum Arbeiten. Darum wird die Mitte (er nennt 50 %) zum Fokus, die Seiten sind Peripherie; die
  Menüleiste wandert nach innen.
- Zieht man ein Fenster an den Rand, **schrumpft** es und lässt sich dort ablegen („stash area“); es bleibt
  auswählbar und bedienbar. Ein fließendes Mitschrumpfen beim Ziehen probierte er, schaltete es aber ab, weil es
  nicht lesbar war.
- Nicht jedes Fenster könne das. Ein Musik-Player baut sich am Rand zu einem Button um (Play/Pause): „a window into
  a widget“.
- Schüttelt man ein Fenster, gehen die anderen automatisch zur Seite.
- Er sagt, Wayland könne das heute; er will einen echten Prototyp bauen.

Das Video ist nur die Anregung; die Umsetzung hier ist Svens eigene Variante für den Tiling-Modus.

## Machbarkeit je System

Nur gelesen, nichts ausprobiert.

| System | Einschätzung | Quelle |
|---|---|---|
| **Hyprland** | Gut machbar. `resizewindowpixel`/`resizeactive` setzen Größen pro Fenster (exakt oder prozentual). Ab 0.54 gibt es Layouts pro Workspace und das Scrolling-Layout im Kern (Spalten mit `layoutmsg colresize`), was dem Bild „Mitte groß, Rand klein“ nahekommt; `splitratio` ist entfallen, dwindle nutzt `default_split_ratio`. In dwindle würde Torro die Verhältnisse selbst pro Fenster setzen. | https://wiki.hypr.land/0.48.0/Configuring/Dwindle-Layout/ · https://hypr.land/news/update54/ · https://github.com/hyprwm/hyprland-wiki/blob/5e1782bd/content/Configuring/Dispatchers.md · https://github.com/hyprwm/Hyprland/pull/12890 · https://github.com/dawsers/hyprscroller (archiviert) |
| **macOS / AeroSpace** | Eingeschränkt. `resize width\|height ± n` wirkt auf einzelne Fenster (`--window-id`), aber im i3-artigen Baum; eine „Breite je Position“-Regel oder max. Fensterbreite gibt es nicht (offene Feature-Anfrage, Discussion #621). Torro müsste Größen selbst per CLI nachführen. AeroSpace nutzt die Accessibility-API; ob Apps jede Größe annehmen, ist hier nicht belegt und muss geprüft werden. | https://nikitabobko.github.io/AeroSpace/commands.html · https://github.com/nikitabobko/AeroSpace/discussions/621 · https://github.com/nikitabobko/AeroSpace/blob/main/docs/guide.adoc |
| **Windows / GlazeWM** | Machbar mit Einschränkung. `resize --width/--height` (px, %, relativ) verändert die Anteile unter Geschwistern, `size` setzt Pixel direkt, `position` verschiebt schwebende Fenster. WebSocket-IPC ist im Repo schon angebunden. Ohne Tiling-Manager (nur Fenster-Modus) wäre das nicht Aufgabe dieses Issues. | https://deepwiki.com/glzr-io/glazewm/2.3-window-management-basics · https://github.com/glzr-io/glazewm/blob/main/README.md |

## Zuständigkeit: Torro oder Window Manager

Alle drei Window Manager verwalten die Größen selbst. Torro kann es nicht allein:
- **Window Manager:** setzt die Größen. Torro braucht dafür das WM-Backend (`torro-wm`), das nach dem Ziehen eines
  Fensters (Ereignis) Größen setzt.
- **Torro-Fenster:** schalten zusätzlich selbst auf die kompakte Größenklasse um, sobald sie schmaler werden.
- **Fremde Fenster:** werden nur in der Größe verändert, nicht in ihrer Darstellung.

Eine reine Torro-Lösung wäre nur der Fenster-Modus mit eigener Arbeitsfläche (dort sind Größen ohnehin Torros
Sache); er ist nicht Gegenstand dieses Konzepts.

## Einordnung im Repo

- **[modes.md](../modes.md):** Die Option gehört zum Tiling-Modus (Hyprland, AeroSpace, GlazeWM).
- **`crates/torro-wm`:** Das Trait `WindowManager` (`src/lib.rs`) kann heute keine Größe setzen; es bräuchte eine
  neue Methode (z. B. `resize`). `Window` (`src/types.rs`) hat schon ein `Rect` (Position und Größe auf dem Desktop).
  Eine neue Trait-Methode ist eine Schnittstellenänderung und kommt nur mit dem freigegebenen Plan.
- **`torro_gui::layout`:** Die Größenklassen oben entscheiden, welche Darstellung ein Torro-Fenster am Rand zeigt.
- **Oberfläche, API, MCP:** Die Einstellung (links, rechts, beide) braucht nach [api.md](../api.md) einen Request mit
  Katalogeintrag und MCP-Tool oder eine Begründung, warum ein Weg fehlt. Das legt der Plan fest.

## Entschieden

Noch nichts. Fest steht nur Svens Wunsch oben (Seiten einstellbar, Fenster am Rand klein, Torro-Fenster mit ihren
Größenklassen).

## Offen

1. Wie erkennt Torro „nach außen ziehen“? Ereignisse gibt es bei Hyprland und GlazeWM, bei AeroSpace nicht (nur
   Callbacks in der Konfiguration, siehe [research.md](../research.md)): dort wäre Polling nötig.
2. Zonen: Wie breit ist die Mitte (im Video 50 %)? Fest, einstellbar oder je Bildschirmbreite?
3. Wie klein genau, und gilt das nur für Torro-Fenster oder auch für fremde Apps?
4. Soll das Mitschrumpfen beim Ziehen animiert sein? (Im Video wurde das verworfen, weil unlesbar.)
5. Reicht die Größenklasse Mini/Narrow am Rand, oder braucht es eine eigene „Widget“-Darstellung wie der
   Musik-Player im Video?
6. Wie kommt ein Fenster zurück (Ziehen zur Mitte, Tastenkürzel)?
7. Zusammenspiel mit Workspace-Vorlagen (Teilungsverhältnisse speichern/laden).
8. Reihenfolge: zuerst Hyprland (hier läuft es), dann GlazeWM, AeroSpace zuletzt?
9. Windows ohne GlazeWM und macOS ohne AeroSpace: nicht unterstützt (Fenster-Modus)?

## Beteiligte

Das ist ein Oberflächen-Thema: **Der Designer** (Agent-ID `e07dfc58-1b1c-4f6c-9d8d-89da4a60ba46`) wird einbezogen,
bevor etwas umgesetzt wird. Es ist nur ein Konzept, es gibt noch keinen Code.
