# Einzelne Apps: jede App ohne Hauptanwendung

Stand: 2026-10-30, entschieden in der Planung von Issue #408 (Themen-Issue mit den Sub-Issues #412–#415).
Konten in den Apps kamen mit #410, Pakete, Release und App-Katalog mit #411 (Abschnitte „Pakete je App“,
„App-Katalog“ und „Mehrere Kopien des Dienstes“).

## Ziel

Jede App der Suite (Mail, Aufgaben, Kalender, Eingang, Assistent) läuft **einzeln, ohne die Hauptanwendung**
und ist **einzeln herunterladbar**. Die Hauptanwendung (`torroworkplace`: Tray-Icon, Steuerzentrale) bleibt das
verbindende Element: Wer sie installiert, kann von dort weitere Apps dazunehmen. Wer nur eine App will, lädt
nur diese und nutzt sie allein.

Mit #279 sind die Apps schon eigene Programme ([lebenszyklus.md](lebenszyklus.md)). Aussehen und Sprache (#414)
und die Konten (#410) zeigen die Apps inzwischen selbst; dieses Dokument hält fest, wie sie sich von der
Hauptanwendung lösen.

## Entschieden in der Planung (2026-10-30)

- **Dienst:** Jede App bringt den Dienst unsichtbar mit. Ohne Hauptanwendung läuft er ohne Tray-Icon und ohne
  Einrichtung der Steuerzentrale; mit Hauptanwendung wie heute. Verworfen: den Dienst in jede App einbauen
  (zwei Kerne auf denselben Daten) und die Hauptanwendung als Pflicht (erfüllt „nur diese App“ nicht).
- **Aufteilung:** drei Themen, siehe unten. #408 ist die Grundstruktur; #410 und #411 werden danach eigens
  geplant.
- **Hauptanwendung installiert weitere Apps** über einen App-Katalog in der Steuerzentrale: Installieren,
  Aktualisieren (auch „Update all“ und die Hauptanwendung selbst) und Entfernen, in einem eigenen Fenster mit
  Fortschritt. Entwurf in #411.
- **Einstellungen:** Konten verbindet jede App selbst (#410, [settings.md](settings.md), „Die Regel“). Aussehen, Sprache und Start beim Anmelden zeigen die
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
3. **Fehlt eine Partner-App, verschwinden nur Sprünge in sie.** Jedes „Open in …“, das
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
4. **Windows:** Der Startmenü-Eintrag `Torro Workplace.lnk` liegt bis zwei Ordner tief unter
   `%APPDATA%\Microsoft\Windows\Start Menu\Programs` oder `%ProgramData%\Microsoft\Windows\Start Menu\Programs`
   (#411; ihn legen das Scoop-Manifest `torroworkplace.json` und der App-Katalog an).

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
| Konten | in der jeweiligen App, Zahnrad › Accounts bzw. AI; die Steuerzentrale › Accounts zeigt alle mit „Used by …“ (#410) | in der jeweiligen App, Zahnrad › Accounts bzw. AI (#410) |
| Was nur eine App betrifft (Kalender, Listen, Erinnerungen, Quellen …) | hinter dem Zahnrad der App ([settings.md](settings.md)) | ebenso |
| Aussehen, Sprache, Start beim Anmelden | Steuerzentrale › Appearance bzw. General | in jeder App, Zahnrad › Appearance bzw. General |
| Hintergrunddienst beenden | Tray › Quit | in jeder App, Zahnrad › General › Background service › „Quit…“ |
| Rechte von API-Schlüsseln und MCP-Clients im Bereich einer App | in der App, Zahnrad › API and MCP; die Steuerzentrale zeigt alle Rechte gesammelt (#531) | in der App, Zahnrad › API and MCP; Schlüssel und Clients anlegen klärt #538 |
| API und MCP anlegen, Freigaben, Workspaces | nur Steuerzentrale | gibt es nicht ohne Hauptanwendung (Anlegen: #538) |

Die Seiten Appearance und General stehen einmal in `torro-settings-ui` (`suite.rs`); Steuerzentrale und Apps
nutzen dieselben Seiten.

## Pakete je App (#411)

Jede App gibt es einzeln, und jede bringt den Dienst mit. Einzelheiten zum Bauen: [release.md](release.md).

| System | Was eine App mitbringt |
|---|---|
| macOS | Ihr Bündel enthält „Torro Workplace Service.app“ unter `Contents/Library/LoginItems/`; signiert und notarisiert wird je Bündel, von innen nach außen. Der Kalender bringt zusätzlich „Torro Reminder.app“ mit. |
| Linux (AUR) | Jedes App-Paket hängt an `torroworkplace-bin` (nur Dienst und systemd-Einheit), Mail und App Feed auch an `torroinbox-bin`. Die Hauptanwendung ist das eigene Paket `torro-workplace` (Starter-Eintrag und Icon), `torro-suite` zieht alle. |
| Homebrew | Jede Formel hängt an `mahype/torro/torroworkplace`, Mail und App Feed auch an `torroinbox`. |
| Windows (Scoop) | Jedes Manifest lädt die eigene `.zip` und `torroworkplace` dazu (Mail und App Feed auch `torroinbox`), Verknüpfung nur für die eigene App. `torroworkplace.json` ist die Hauptanwendung, `torro-suite.json` alle. |
| Linux (direkte DEB/RPM/pacman, #589) | Acht Pakete je Format/Architektur: Dienst `torroworkplace-bin`, Steuerzentralen-Starter `torro-workplace`, sechs App-Pakete. Dienst mindestens gleicher Version, Mail/App Feed auch Hub. Keine neue Paketquelle. |
| Windows (Setup, #589) | Systemweiter Installer je App: eigener Starter/Uninstaller, Dienst und Hub gemeinsam und referenzgezählt; Administratorrechte, keine erhöhten App-Starts oder Datenlöschung. |
| macOS (DMG, #589) | App-Bündel mit Dienst; Mail/App Feed zusätzlich Hub, Kalender Reminder. Alle angezeigten Apps nach Applications ziehen. Image und Bündel signiert, notarisiert und geheftet. |

Die alten Programmarchive und Skriptinstaller im Release bleiben ohne Dienst: Wer unter Linux oder Windows eine App so von
Hand lädt, braucht `torroworkplace` dazu. Diese Archive legen auch keinen Starter-Eintrag bzw. keine
Startmenü-Verknüpfung an; ohne sie gilt die Hauptanwendung als nicht installiert, und der Dienst läuft still.

Die nativen Downloads stehen samt Begleitpaketen im Release-Index `downloads.md`. Anleitung:
[install.md](install.md). Der echte Release- und Plattformnachweis für #589 steht noch aus.

## Mehrere Kopien des Dienstes (#411)

Bringt jede App den Dienst mit, liegen oft mehrere Kopien auf dem Rechner. Es startet immer die neueste, und eine
ältere arbeitet nie auf Daten, die eine neuere schon benutzt hat ([lebenszyklus.md](lebenszyklus.md),
Entscheidung 10):

- **Welche startet:** macOS die Kopie mit der höchsten Version aus den Bündeln aller Apps (neben dem eigenen Bündel,
  in `/Applications`, in `~/Applications`); bei Gleichstand die in „Torro Workplace.app“, dann die im eigenen Bündel.
  Unter Scoop bringt jede App eine eigene Kopie mit (`<scoop>/apps/<app>/current/torroworkplace.exe`); dort startet
  die mit der höchsten Version laut `manifest.json`. AUR und Homebrew haben nur die eine Kopie aus dem Paket.
- **Entfernen:** Liegt der laufende Dienst im Bündel der App, die entfernt werden soll, und ist jede verbleibende
  Kopie älter, lehnt der Katalog ab („update Torro Workplace first“): Die ältere Kopie würde die Daten verweigern,
  und es bliebe kein Dienst.
- **Datenschutz:** `<state>/service-version` hält die höchste Version, die auf den Daten lief. Ein älterer Dienst
  startet nicht und nennt beide Versionen.
- **Ablösen:** Nach einer Installation oder einem Update mit neuerer Kopie startet der laufende Dienst die neue mit
  `daemon --takeover` und endet ohne Quit-Marke.
- **Start beim Anmelden** zeigt nach jedem Start auf den laufenden Dienst.

## App-Katalog (#411)

Der Abschnitt **Apps** der Steuerzentrale (gleich nach Overview, `--section apps`) zeigt die Hauptanwendung und jede
App mit Icon, Frage, Beschreibung und Version: installierte oben, fehlende darunter. Entwurf:
[prototypes/app-catalog/index.html](prototypes/app-catalog/index.html); wo der Plan zu #411 abweicht, gilt der Plan
(kein Sortieren, keine Listenansicht, kein „Reinstall“, keine Karte „Coming soon“). Nach der Design-Prüfung
abweichend vom Plan: In mittlerer Breite stehen die Karten zweispaltig, sobald zwei Karten nebeneinander passen; die
Version steht in der unteren Zeile der Karte, „Release notes“ nur im Menü „…“.

Entschieden mit dem Plan zu #411:

1. **Installieren wie die Hauptanwendung.** Kam sie über AUR, Homebrew oder Scoop (erkannt am Pfad des Programms),
   zeigt jede Karte „Managed by …“ mit dem Befehl zum Kopieren, statt selbst zu installieren. Sonst installiert der
   Katalog selbst; auf macOS immer selbst.
2. **Prüfen ohne Schalter:** Der Dienst fragt 60 s nach dem Start, dann täglich und beim Öffnen von Apps die
   Releases auf GitHub (`releases/latest`). Innerhalb von fünf Minuten antwortet er aus dem letzten Stand (Grenze von
   GitHub). Ohne Netz bleibt der letzte Stand mit dem Hinweis „Couldn't reach GitHub.“
3. **Ein/Aus** einer App (die Komponente) steht im Menü „…“ ihrer Karte; eine ausgeschaltete App zeigt „Turned off“.
4. **Mail und App Feed bringen den Notification Hub mit,** weil sie dessen Programm zum Verfassen brauchen. Den Hub
   entfernen geht erst, wenn beide fehlen. Verworfen: das Verfassen in Mail einbauen (baut den Eingang um).
5. **Ein Update ersetzt eine laufende App sofort;** offene Fenster nutzen die neue Version nach dem Schließen.
   Verworfen: auf das Schließen warten, weil die Kacheln dauernd offen sind.

Installieren, Aktualisieren und Entfernen laufen als **Auftrag** im Dienst, einer zur Zeit, mit den Schritten
Download, Check signature (macOS: `codesign`, Team `2GA7DQ3P3Z`; sonst SHA-256), Install und Connect to the service.
Abbrechen geht nur beim Download. Die Steuerzentrale zeigt den Auftrag in einem kleinen eigenen Fenster ohne Rahmen
(`torroworkplace ui --app-job=<id>`); Entfernen fragt vorher, ob auch die Daten der App auf diesem Rechner gehen
(Konten bleiben, andere Apps nutzen sie). Entfernte Apps gehen in den Papierkorb. „Show in Finder“ (Explorer,
Ordner) öffnet den Ort, den der Dienst im Katalog nennt (`path`), lokal ohne Befehl, wie das Öffnen eines Ordners
unter Appearance: Es liest und ändert nichts. Das Update der Hauptanwendung
startet danach den Dienst neu; das Fenster des Auftrags wartet auf ihn und holt die Steuerzentrale nach vorn. Eine
offene Steuerzentrale verbindet sich neu und nutzt wie jedes offene Fenster die neue Version, sobald sie geschlossen
wird (Entscheidung 5). In Entwicklungs-Builds antworten Installieren, Aktualisieren und
Entfernen mit `unsupported`.

**Begrenzte Ausnahme #589:** DEB/RPM/pacman/Windows-Setup werden über paket-eigene Marker neben den
Programmen erkannt (Schema, Manager, Paket, kanonischer Programmpfad). Beschädigte Marker sperren
Änderungen ebenfalls. Vor jedem Auftrag werden alle Zielobjekte einschließlich fehlender Abhängigkeiten
und tatsächlicher Zielpfade geprüft; ein verwaltetes Ziel liefert `conflict`, ohne Teiländerung.
Die Herkunft des Dienstes gilt nur für fehlende Apps, nicht für vorhandene fremde Kopien. Native
Windows-Verzeichnisse werden auch vom `.old`-Aufräumen ausgenommen. Installieren/Aktualisieren verweist
auf Downloads, Entfernen auf apt/dnf/pacman bzw. `control appwiz.cpl`. Bestehendes Layout und
API/MCP-Rechte bleiben; Installieren/Aktualisieren weiterhin `Never`. Ohne Marker gelten weiter die
bisherigen Manager und selbstverwalteten Installationen; macOS wird wie bisher selbst verwaltet.

## Oberfläche, API, MCP

| Funktion | Oberfläche | API | MCP |
|---|---|---|---|
| Ist die Hauptanwendung installiert? | Apps entscheiden danach, was sie zeigen | `apps.list_apps`, Feld `main_app` | `apps_list_apps` |
| Aussehen, Sprache, Start beim Anmelden | Steuerzentrale bzw. App | `settings.set_setting`, Listen der Designs, Schriften, Farbschemata | `settings_*` (vorhanden) |
| Apps mit Versionen, auf Updates prüfen | Abschnitt Apps | `apps.list_catalog`, `apps.check_updates` | `apps_list_catalog`, `apps_check_updates` |
| Stand eines Auftrags, Abbrechen | Fenster des Auftrags | `apps.app_job`, `apps.cancel_app_job` | `apps_app_job`, `apps_cancel_app_job` |
| App entfernen | Menü „…“ › Uninstall… | `apps.uninstall_app` (Admin) | `apps_uninstall_app`, immer mit Freigabe |
| App installieren, aktualisieren (auch „Update all“ und die Hauptanwendung) | Knopf der Karte, „Update all“ | `apps.install_app`, `apps.update_apps` (nur App-Schlüssel) | keins: Wer installiert, entscheidet, welcher Code läuft (Punkt 11 in „Was nie nach außen geht“, [api.md](api.md)) |
| Dienst beenden | Tray › Quit bzw. App › General › Background service › „Quit…“ | `system.quit` (nur App-Schlüssel) | keins, siehe „Was nie nach außen geht“ in [api.md](api.md) |
