# Kern: Tray-Icon und Workspace-Vorlagen

Stand: 2026-10-20. Konzept zur Freigabe. Der Prototyp ist gebaut; Tasten, Laden beim Start und der
Dialog der Vorlagen kamen mit #243 dazu (Aufgaben WS.1–WS.8 in [tasks/workspaces.md](tasks/workspaces.md)).

Der Kern `torroworkplace` ist die Basis der Suite. Er sitzt als Tray-Icon oben rechts in der
Leiste und hält die Arbeitsbereiche des Tiling-Window-Managers fest: Er speichert einen
Workspace als **Vorlage** und baut ihn auf Knopfdruck wieder auf. Kalender, Aufgaben und alle
weiteren Apps bauen später auf diesem Kern auf.

Gebaut wird direkt in Rust. Das Omarchy-Plugin
[workspace-setups](../apps/workspace-setups/README.md) dient nur als **Referenz**. Es zeigt, was
funktioniert und welche Funktionen wir später haben wollen. Übernommen wird daraus kein Code.

## Plattformen

Pro System gibt es genau einen Tiling-Window-Manager, den der Kern unterstützt.

| | Linux | macOS | Windows |
|---|---|---|---|
| **Tiling-Manager** | Hyprland (Omarchy 4) | AeroSpace (Public Beta, v0.21) | GlazeWM (v3) |
| **Tray-Icon** | `ksni` (StatusNotifierItem über D-Bus, ohne GTK); erscheint im Tray der Omarchy-Leiste (Quickshell) | `tray-icon` (Menüleiste) | `tray-icon` (Infobereich) |
| **Abfragen und Steuern** | `hyprctl -j` bzw. Befehls-Socket | CLI `aerospace … --json` | WebSocket `ws://127.0.0.1:6123`, `query` und `command` |
| **Ereignisse** (Fenster auf/zu, Workspace gewechselt) | Ereignis-Socket `.socket2.sock` | keine Ereignisschnittstelle, nur Callbacks in der Konfiguration; deshalb Abfragen in kurzen Abständen | `sub` über denselben WebSocket |
| **Layoutmodell** | dwindle: Binärbaum mit Teilungsverhältnissen (`preselect`, `splitratio`) | i3-artiger Baum aus Kacheln und Akkordeons (`join-with`, `layout`, `resize`) | i3-artiger Baum (`set-tiling-direction`, `resize`) |
| **Tastenkürzel** | Zur Laufzeit über `hl.bind` (Lua unter Omarchy 4), vom Kern gesetzt | `~/.aerospace.toml` | GlazeWM-Konfiguration |
| **Autostart** | `exec-once` oder systemd-User-Dienst | LaunchAgent | Autostart-Eintrag |

Warum diese drei: Hyprland ist gesetzt. AeroSpace braucht kein Abschalten des
Systemschutzes (SIP), wird aktiv gepflegt und hat eine JSON-CLI; yabai bräuchte für Spaces
teilweise abgeschaltetes SIP. GlazeWM steht unter GPL-3, ist in Rust geschrieben und hat eine
saubere WebSocket-Schnittstelle. komorebi ist technisch stark, erlaubt aber nur private Nutzung;
wer es beruflich einsetzt, braucht eine kostenpflichtige Lizenz. Details in [research.md](research.md).

**Globale Tastenkürzel** für die Vorlagen setzt der Kern selbst, weil eine App unter Wayland keine
globalen Tasten belegen darf. Unter Hyprland geschieht das über die Lua-Schnittstelle des Tiling-
Managers (`hl.bind`, `hl.unbind`): zur Laufzeit, nach dem Start und nach jedem Neuladen der
Konfiguration erneut; die Konfigurationsdatei bleibt unberührt. Die Taste ruft dann
`torroworkplace ctl setups load <id> --mode add` auf. Unter macOS und Windows meldet der Dienst die
Tasten über das Crate `global-hotkey` an; ein Druck lädt die Vorlage wie „Hinzufügen“ im Menü.

## Funktionen

Die Liste orientiert sich an workspace-setups. **P** = im Prototyp, **S** = später.

| Funktion | |
|---|---|
| Tray-Icon mit Menü | P |
| Aktiven Workspace als Vorlage speichern | P |
| Vorlage in den aktiven Workspace laden; ist er belegt: hinzufügen oder ersetzen | P |
| Kachelbaum mit Teilungsverhältnissen erfassen und wieder aufbauen | P |
| Startbefehl je Fenster ermitteln | P |
| Strategie je Fenster: **neu** öffnen oder offenes Fenster **herholen** | P |
| Strategie **automatisch** (starten, sonst herholen) | S |
| Vorlage aktualisieren, löschen | P |
| Dialog „Workspace speichern“ und „Vorlage bearbeiten“: Name, Taste, Start | P, #243 |
| Vorlage benennen und umbenennen (im Dialog der Steuerzentrale und im eigenen Dialogfenster) | P, #243 |
| Fortschritt und Ergebnis als Desktop-Benachrichtigung | P |
| Fortschritt mit Abbruch | S |
| Floating, Vollbild, angepinnte Fenster | S |
| Tasten 1 … 0 zum Laden, Modifikator einstellbar; die F1–F10 aus workspace-setups sind verworfen | P, #243 |
| Beim Anmelden laden: einmal pro Anmeldung, je Vorlage auf einen Bildschirm (fehlt er: Hauptbildschirm oder auslassen) | P, #243 |
| Terminal: Arbeitsverzeichnis und interaktives Programm (tmux, nvim, lazygit …) | S |
| Browser-Tabs (Chromium, Chrome, Brave) | S |
| Vorlagen aus workspace-setups (Schema 2) einlesen | S |

## Tray-Menü im Prototyp

Ein Tray-Menü kann keinen Text abfragen und keine Dialoge zeigen. „Workspace speichern…“ speichert deshalb
sofort unter einem automatischen Namen (z. B. „Workspace 3, 11.10. 14:20“) und öffnet danach den Dialog, in dem
Name, Taste und Start stehen. „Abbrechen“ löscht die gerade gespeicherte Vorlage wieder. Ist der aktive Workspace
belegt, fragt das Menü nicht nach, sondern bietet die Wahl direkt als Untermenü an.

```
Torro Workplace
────────────────────────────
Workspaces · Hyprland        (nur Anzeige)
Entwicklung · 1           ▸
Kommunikation · 0         ▸
Notizen                   ▸
────────────────────────────
Workspace speichern…
Workspaces verwalten…
────────────────────────────
Einstellungen…
Beenden
```

Jede Vorlage hat ein Untermenü mit „Hier laden“, „Hinzufügen“, „Ersetzen“ und „Bearbeiten…“ (öffnet den
Dialog). „Hier laden“ ist nur aktiv, wenn der Workspace leer ist. Vorlagen mit Taste tragen die Ziffer hinter
dem Namen („· 1“), weil Tray-Menüs keine rechtsbündigen Hinweise kennen. „Aktualisieren“ und „Löschen“ gibt es
nur auf der Seite Workspaces der Steuerzentrale. Ein Linksklick auf das Icon öffnet das Menü. Die Einträge
bleiben kurz, weil die Omarchy-Leiste Tray-Menüs nur etwa 232 px breit zeichnet. Später kommen
„Steuerzentrale öffnen …“ und die Apps hinzu.

## Prozesse

| Prozess | Aufgabe |
|---|---|
| `torroworkplace daemon` | Tray-Icon, Vorlagen, Anbindung an den Tiling-Manager, IPC-Socket. Läuft genau einmal pro Sitzung |
| `torroworkplace ctl …` | Befehle von außen, z. B. für Tastenkürzel: `ctl setups load Entwicklung --mode add` |
| `torroworkplace status --json` | Kurzstatus für Leisten und Skripte |
| `torroworkplace ui` (später) | Steuerzentrale als eigenes Iced-Fenster, spricht über IPC mit dem Dienst |

Im Dienst gehört der **Haupt-Thread dem Tray-Icon**, weil macOS das verlangt. Alles andere läuft
in einer tokio-Laufzeit daneben: IPC, Ereignisse des Tiling-Managers, Laden und Speichern. Beide
Seiten sprechen über Kanäle miteinander, und das Menü wird bei jeder Zustandsänderung neu gebaut.

Tray-Menü, `ctl`, Tastenkürzel, später die Steuerzentrale und MCP rufen **dieselben Befehle**
im Dienst auf. Die Logik gibt es nur einmal.

Die Steuerzentrale läuft getrennt, damit der Dienst leicht bleibt und auf dem Mac Tray-Icon und
GUI nicht um den Haupt-Thread streiten.

## Crates

| Crate | Inhalt | |
|---|---|---|
| `torro-core` | App-IDs, Pfade pro OS, Theme-Tokens | vorhanden |
| `torro-wm` | Trait `WindowManager`, neutrale Typen (Fenster, Workspace, Monitor, Rechteck), Backends `hyprland`, `aerospace`, `glazewm`, Erkennung zur Laufzeit | **neu** |
| `torro-setups` | Vorlagen (Schema 3), Erfassen, Laden mit Strategien und Fensterzuordnung, Speicherung, Startbefehle je OS | **neu** |
| `torro-ipc` | Protokoll zwischen Dienst und Clients (JSON-RPC über lokalen Socket) | vorhanden, wird gefüllt |
| `torroworkplace` | Dienst, Tray (`ksni` unter Linux, `tray-icon` sonst), `ctl`, `status` | vorhanden, wird gefüllt |
| `torro-pim`, `torro-pim-caldav`, `torro-tui`, `torrotasks`, `torrocalendar` | Apps und Daten | ruhen bis nach dem Kern |

### Der Trait `WindowManager` (Entwurf)

```rust
trait WindowManager {
    fn name(&self) -> &'static str;                  // "hyprland", "aerospace", "glazewm"
    async fn active_workspace(&self) -> Result<WorkspaceId>;
    async fn windows(&self, workspace: Option<&WorkspaceId>) -> Result<Vec<Window>>;
    async fn focus_window(&self, window: &WindowId) -> Result<()>;
    async fn move_to_workspace(&self, window: &WindowId, workspace: &WorkspaceId) -> Result<()>;
    async fn close_window(&self, window: &WindowId) -> Result<()>;
    /// Vor dem Öffnen eines Fensters: wohin es neben `sibling` kommen soll.
    async fn prepare_insert(&self, sibling: &WindowId, split: Split) -> Result<()>;
    /// Nach dem Erscheinen: an die Stelle schieben und das Verhältnis setzen.
    async fn place(&self, window: &WindowId, sibling: &WindowId, split: Split) -> Result<()>;
    fn events(&self) -> impl Stream<Item = WmEvent>;  // Fenster auf/zu/verschoben, Workspace gewechselt
}
```

`Window` enthält ID, App-Kennung (Klasse, Bundle-ID oder Programmpfad), Titel, PID, Rechteck,
Floating und Vollbild. Unter Hyprland erledigt `prepare_insert` die Arbeit (`preselect`) und
`place` setzt nur das Verhältnis. Unter AeroSpace und GlazeWM ist es eher umgekehrt.

### Layout erfassen und aufbauen

- **Erfassen geht für alle gleich:** Aus den Fensterrechtecken des Workspace entsteht ein
  Binärbaum aus Teilungen mit Richtung und Verhältnis. So macht es workspace-setups heute schon,
  und es braucht keine Kenntnis des internen Baums des Tiling-Managers.
- **Aufbauen** geschieht Fenster für Fenster in Baum-Reihenfolge: vorbereiten, starten oder
  herholen, auf das Fenster warten, platzieren. Jedes Backend setzt das so genau um, wie sein
  Tiling-Manager es zulässt. Unter AeroSpace stimmen die Verhältnisse nur ungefähr.

### Vorlagenformat (Schema 3)

Eine Vorlage speichert den Baum, die Fenster mit Strategie und Startbefehl, den Quell-Monitor
und neu das Feld `wm`. Die App-Kennung ist je OS verschieden: Klasse und `.desktop`-ID unter
Linux, Bundle-ID auf dem Mac, Programmpfad unter Windows. Vorlagen gelten deshalb pro Rechner;
sie zwischen Systemen zu teilen ist kein Ziel. Ablage: `templates/*.json` im Datenverzeichnis des
Kerns, mit Sicherungskopien.

Seit #243 hat eine Vorlage zwei optionale Felder. Das Format bleibt Schema 3; Dateien ohne die Felder
laden unverändert.

- `shortcut`: die Ziffer 0 bis 9, mit der die Vorlage über die Tasten lädt (der Modifikator kommt aus den
  Einstellungen). Eine Taste gehört höchstens einer Vorlage; wer sie setzt, nimmt sie der anderen weg.
- `startup`: `screen` nennt den Bildschirm beim Namen des Fenstermanagers (`DP-1`), `fallback` sagt, was
  beim Anmelden geschieht, wenn er fehlt: `main` (Vorgabe) lädt auf den Hauptbildschirm, `skip` lädt
  nicht.

```json
"shortcut": 1,
"startup": { "screen": "DP-1", "fallback": "main" }
```

Die F1–F10 der alten Python-App (`SUPER + F1` … `F10`) sind verworfen. Es gibt die Tasten 1 … 0.

## Prototyp: Umfang und Abnahme

Fertig ist der Prototyp, wenn unter Omarchy:

1. `torroworkplace daemon` sein Icon im Tray der Omarchy-Leiste zeigt,
2. das Menü die Vorlagen auflistet und „Aktiven Workspace speichern“ eine Vorlage anlegt,
3. „Laden“ die Fenster mit ihren Teilungsverhältnissen im aktiven Workspace wieder aufbaut
   (leer, hinzufügen, ersetzen),
4. `torroworkplace ctl setups load <Name>` dasselbe von der Kommandozeile tut,
5. das Ganze in der CI auch für macOS und Windows baut. Dort erscheint das Icon, und das Menü meldet
   „AeroSpace/GlazeWM noch nicht unterstützt“, bis die Backends folgen.

Danach in dieser Reihenfolge: Steuerzentrale (Namen, Einstellungen), Tastenplätze,
Wiederherstellen beim Anmelden, Backend AeroSpace, Backend GlazeWM, dann die Apps.

## Vorab zu klären (kurze Versuche)

1. Zeigt der Tray der Omarchy-Leiste das `ksni`-Menü samt Untermenüs korrekt an? Das ist der erste Versuch.
2. AeroSpace: Lässt sich ein Baum mit `join-with`, `layout` und `resize` zuverlässig aufbauen?
3. GlazeWM: Gilt dasselbe mit `set-tiling-direction` und `resize`?
4. Wie ermitteln wir unter macOS und Windows den Startbefehl und ordnen neue Fenster beim Laden
   zu? Unter Linux geht das über die PID-Abstammung und `/proc`.
