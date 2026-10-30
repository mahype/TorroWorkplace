# Einzelne Apps: jede App ohne Hauptanwendung

Stand: 2026-10-30, entschieden in der Planung von Issue #408 (Themen-Issue mit den Sub-Issues #412–#415).
Konten folgen mit #410, Pakete, Release und App-Katalog mit #411; was dort noch zu planen ist, steht unter
„Drei Themen“.

## Ziel

Jede App der Suite (Mail, Aufgaben, Kalender, Eingang, Assistent) läuft **einzeln, ohne die Hauptanwendung**
und ist **einzeln herunterladbar**. Die Hauptanwendung (`torroworkplace`: Tray-Icon, Steuerzentrale) bleibt das
verbindende Element: Wer sie installiert, kann von dort weitere Apps dazunehmen. Wer nur eine App will, lädt
nur diese und nutzt sie allein.

Mit #279 sind die Apps schon eigene Programme ([lebenszyklus.md](lebenszyklus.md)). Konten, Aussehen, Sprache
und der Dienst hängen aber noch an der Hauptanwendung; dieses Dokument hält fest, wie sie sich lösen.

## Entschieden in der Planung (2026-10-30)

- **Dienst:** Jede App bringt den Dienst unsichtbar mit. Ohne Hauptanwendung läuft er ohne Tray-Icon und ohne
  Einrichtung der Steuerzentrale; mit Hauptanwendung wie heute. Verworfen: den Dienst in jede App einbauen
  (zwei Kerne auf denselben Daten) und die Hauptanwendung als Pflicht (erfüllt „nur diese App“ nicht).
- **Aufteilung:** drei Themen, siehe unten. #408 ist die Grundstruktur; #410 und #411 werden danach eigens
  geplant.
- **Hauptanwendung installiert weitere Apps** über einen App-Katalog in der Steuerzentrale: Installieren,
  Aktualisieren (auch „Update all“ und die Hauptanwendung selbst) und Entfernen, in einem eigenen Fenster mit
  Fortschritt. Entwurf in #411.
- **Einstellungen:** Konten ziehen in die Apps (#410). Aussehen, Sprache und Start beim Anmelden zeigen die
  Apps selbst, solange die Hauptanwendung fehlt (#414). API, MCP, Freigaben und Workspaces bleiben in der
  Hauptanwendung.
- **Lose Kopplung:** Die Logik bleibt im Dienst. Alles, was Daten anlegt (etwa eine Aufgabe aus einer Mail),
  geht deshalb auch ohne Partner-App; nur Sprünge in eine fehlende App werden ausgeblendet (#415).

## Entscheidungen

1. **Tray und Einrichtung nur mit Hauptanwendung.** Der Dienst zeigt Tray-Icon und Einrichtung nur, wenn die
   Hauptanwendung installiert ist, egal wer ihn startet (App, MCP-Server, Anmeldung, Terminal). Sonst läuft er
   still wie mit `--headless`. Verworfen: den Startweg entscheiden lassen (still, wenn eine App ihn startet).
   Der Start beim Anmelden brächte sonst doch ein Tray-Icon.
2. **Suiteweite Einstellungen in der App nur ohne Hauptanwendung.** Fehlt sie, zeigt jede App hinter dem Zahnrad
   die Abschnitte **Appearance** (Hell/dunkel, Design, Schrift, Farbschema, Zoom) und **General** (Language,
   Start at login, Background service mit „Quit background service“ und dem Knopf „Quit…“, der erst
   nachfragt), hinter einer Linie nach den eigenen Abschnitten. Hat eine App schon einen Abschnitt „General“
   (der Assistent), heißt der der Suite dort „All apps“ (`--settings suite-general`). `--settings appearance`
   bzw. `general` öffnet sie direkt. Ist die Hauptanwendung da, bleiben die Verweise „Appearance ↗“ und
   „All settings ↗“ wie heute. Verworfen: die Abschnitte immer in jeder App zeigen. Eine Einstellung stünde
   sonst an zwei Orten.
3. **Fehlt eine Partner-App, verschwinden nur Sprünge in sie.** Der App-Umschalter und jedes „Open in …“, das
   nur eine andere App öffnet (`apps.open_app` an eine Partner-App), fehlen, wenn das Ziel nicht installiert
   ist. Was Daten anlegt, etwa eine Aufgabe aus einer Mail, bleibt, weil die Daten im Konto landen.
   Verworfen: auch das Anlegen ausblenden.

## Drei Themen

| Thema | Inhalt |
|---|---|
| **#408** Grundstruktur | #412 dieses Konzept · #413 Dienst ohne Hauptanwendung ohne Tray und Einrichtung · #414 Aussehen, Sprache und Dienst in den Einstellungen der Apps · #415 Sprünge in fehlende Apps ausblenden |
| **#410** Konten und Einrichtung in den Apps | Konten der jeweiligen App (Mail-Konten in Mail statt in der Steuerzentrale), Einrichtung beim ersten Start einer einzelnen App |
| **#411** Paketierung, Release und App-Katalog | ein Paket je App und Plattform, Release-Skript für alle Einzel-Apps (nur bei einem Tag, [release.md](release.md)), App-Katalog in der Steuerzentrale mit Installieren, Aktualisieren und Entfernen, Erkennung der Hauptanwendung unter Windows |

## Ist die Hauptanwendung installiert?

Der Dienst entscheidet das selbst (`apps::main_app_installed`), der Reihe nach:

1. `TORRO_MAIN_APP=1` → ja, `TORRO_MAIN_APP=0` → nein; jeder andere Wert zählt nicht.
2. **macOS:** Das Bündel „Torro Workplace.app“ ist zu finden (`torro_gui::source::bundle_program`: neben dem
   eigenen Bündel, in `/Applications`, in `~/Applications`).
3. **Linux und übriges Unix:** `applications/torro-workplace.desktop` liegt unter `$XDG_DATA_HOME` (leer oder
   ungesetzt: `$HOME/.local/share`) oder unter einem Eintrag von `$XDG_DATA_DIRS` (leer oder ungesetzt:
   `/usr/local/share:/usr/share`).
4. **Windows:** gilt als installiert, bis #411 die Pakete je App bringt.

`apps.list_apps` meldet das Ergebnis im Feld `main_app`. Fehlt das Feld (ein älterer Kern), gilt `true`, damit
nichts umschaltet. Die Apps lesen `main_app` **nur aus der Antwort des Dienstes**: Startet der Dienst eine App über
ihr Bündel (`open`), reicht er keine Umgebung weiter, eine Variable in der App wäre also unzuverlässig. Bis zur
ersten Antwort gilt die Hauptanwendung als installiert. Nur mit `--demo` wirkt `TORRO_MAIN_APP=0` direkt in der
App, für die Sichtprüfung.

**Entwicklungs-Builds:** Ohne installierte Hauptanwendung (kein „Torro Workplace.app“ in den Programmen, unter
Linux kein Starter-Eintrag) läuft ein Dienst aus `cargo run` ohne Tray-Icon, auch beim Start über die
Hyprland-Konfiguration. Wer so arbeitet, setzt `TORRO_MAIN_APP=1` dort, wo der Dienst startet.

## Was wo eingestellt wird

| Einstellung | mit Hauptanwendung | ohne Hauptanwendung |
|---|---|---|
| Konten | Steuerzentrale › Accounts; mit #410 in der jeweiligen App | in der jeweiligen App (#410) |
| Was nur eine App betrifft (Kalender, Listen, Erinnerungen, Quellen …) | hinter dem Zahnrad der App ([settings.md](settings.md)) | ebenso |
| Aussehen, Sprache, Start beim Anmelden | Steuerzentrale › Appearance bzw. General | in jeder App, Zahnrad › Appearance bzw. General |
| Hintergrunddienst beenden | Tray › Quit | in jeder App, Zahnrad › General › Background service › „Quit…“ |
| API, MCP, Freigaben, Workspaces | nur Steuerzentrale | gibt es nicht ohne Hauptanwendung |

Die Seiten Appearance und General stehen einmal in `torro-settings-ui` (`suite.rs`); Steuerzentrale und Apps
nutzen dieselben Seiten.

## Oberfläche, API, MCP

| Funktion | Oberfläche | API | MCP |
|---|---|---|---|
| Ist die Hauptanwendung installiert? | Apps entscheiden danach, was sie zeigen | `apps.list_apps`, Feld `main_app` | `apps_list_apps` |
| Aussehen, Sprache, Start beim Anmelden | Steuerzentrale bzw. App | `settings.set_setting`, Listen der Designs, Schriften, Farbschemata | `settings_*` (vorhanden) |
| Dienst beenden | Tray › Quit bzw. App › General › Background service › „Quit…“ | `system.quit` (nur App-Schlüssel) | keins, siehe „Was nie nach außen geht“ in [api.md](api.md) |
