# Release-Pipeline

Stand: 2026-11-13, #589. **Noch kein veröffentlichter Installer-Nachweis.** Der Code bereitet die
Downloads vor; erst ein Maintainer-Release mit Plattformprüfungen belegt die vollständige Auslieferung.
Anleitung für Nutzer: [install.md](install.md). Maintainer-Ablauf: [release-0.1.0.md](release-0.1.0.md).

## Artefakte und Begleitpakete

`packaging/apps.json` nennt die acht stabilen App-IDs, Programme, Startargumente und Abhängigkeiten:
Workplace, Notification Hub, Workplace Mail, App Feed, Calendar, Tasks, Assistant und Chat. Bundle-ID bleibt
`io.github.mahype.<app-id>`, die des Dienstes `io.github.mahype.torro-workplace-service`.

| Plattform | Installierbarer Download | Inhalt |
|---|---|---|
| macOS arm64 / Intel | `<bin>-<version>-<target>.dmg` | signierte, notarisierte, geheftete App; Mail/App Feed zusätzlich Hub, Calendar zusätzlich Reminder; jede App mit eingebettetem Dienst |
| Windows x64 | `<bin>-<version>-x86_64-pc-windows-msvc-setup.exe` | unsignierter Inno-Setup-Installer für alle Benutzer, App, Dienst, bei Mail/App Feed Hub |
| Linux amd64 / arm64 | native nFPM-Namen, `.deb`, `.rpm`, `.pkg.tar.zst` | neun Pakete je Ziel und Format: Dienst, Workplace-Starter und sieben Apps |

Linux besitzt `torroworkplace-bin` allein das Programm `/usr/bin/torroworkplace` und
`/usr/lib/systemd/user/torroworkplace.service`. `torro-workplace` besitzt nur Starter und Icon. Die sieben
`<bin>-bin`-Pakete besitzen jeweils Programm, Paketmarker, Starter und Icon. Jedes App-Paket braucht den
Dienst mindestens derselben nativen Version; Mail/App Feed auch `torroinbox-bin`. Kein doppelter
Dateibesitz, keine Root-Dienststarts, kein automatisches Aktivieren der Nutzereinheit oder Autostart.

Die Pipeline verlangt **78 Installer**: 54 Linux-Pakete, 16 DMGs und acht Setups. Dazu gehören je Datei
`.sha256`, ein maschinenlesbares `installers-manifest.json` und `downloads.md` mit eindeutig zugeordneten
Links einschließlich Begleitpaketen derselben Architektur und desselben Formats. Die Downloadtabelle
steht auch in den Release-Notizen. Native Linux-Namen kommen direkt von nFPM, nicht von geratenen Namen.

Archive bleiben unverändert für Katalog, AUR, Homebrew und Scoop:

| Ziel | Archiv |
|---|---|
| Linux, beide Architekturen | `<bin>-<target>.tar.xz`, Ordner `<bin>-<target>/` mit Programm, Lizenzen, README |
| Windows | `<bin>-x86_64-pc-windows-msvc.zip`, Programm/Lizenzen/README ohne gemeinsamen Ordner |
| macOS, Terminal | `<bin>-<target>.tar.xz` |
| macOS, Katalog | `<bin>-<target>.app.zip`; außerdem `torro-reminder-<target>.app.zip` |

Dazu weiterhin `<bin>-installer.sh`, `<bin>-installer.ps1`, `<bin>.rb`, `sha256.sum`, `source.tar.gz`.
Diese **Programmarchive/Skriptinstaller** sind nicht die benutzerfreundlichen nativen Downloads und
bringen unter Linux/Windows nicht automatisch Dienst, Starter oder Begleitprogramme mit.

## Ablauf und harte Gates

Nur ein Versions-Tag startet `release.yml`, keine PR- oder manuellen Trigger der Installer-Workflows.
Versionen laufen gemeinsam: `vMAJOR.MINOR.PATCH` oder `vMAJOR.MINOR.PATCH-rc.N`. Die Installer akzeptieren
keine anderen Tagformen; Komponenten sind wegen Windows auf 65534 begrenzt.

1. `dist` **0.32.0** plant alle acht Programme für fünf Ziele. `custom-ci` prüft Linux, macOS und Windows.
2. `build-local-artifacts` baut auf Ubuntu 22.04 (x86_64/arm64), macOS 14 (arm64), macOS 15 Intel und
   Windows 2022 die Archive. Linux-Basis: glibc **>= 2.35**, dynamische Grafikloader für X11/Wayland.
3. `build-global-artifacts` erzeugt die bisherigen Skriptinstaller, Formeln und Gesamtprüfsummen.
4. Globale Artefakt-Jobs (auch bei RCs):
   - `publish-macos-app.yml::macos-app` lädt **Workflow-Artefakte**, prüft Tag und SHA-256, baut die
     Bündel mit `make-app.sh`, signiert innen nach außen, notarisiert und heftet. Ein Folgejob baut mit
      `make-dmg.sh` acht Images je Architektur: Applications-Link, englische Ziehanleitung, benötigte
     Partner-Bündel. Auch DMGs werden signiert, notarisiert, geheftet und validiert. Temporäre Mounts
     werden aufgeräumt. App-Archivnamen, Dienst und Bundle-IDs bleiben unverändert.
   - `build-installers.yml` nutzt nFPM **2.48.0** und Inno Setup **7.1.1**, deren Download-SHA-256 im
      Workflow feststeht. `installers.py` prüft die acht Quellen im dist-Plan, Tag und Quellprüfsummen,
     lehnt Pfadausbrüche, Links und Sonderdateien ab und arbeitet temporär. Tool-/Eingabefehler liefern
     Exit ungleich 0, keine teilweise fertige Ausgangsmatrix.
5. `host` verlangt **Erfolg aller Pflichtjobs** einschließlich `custom-ci`; „skipped“ reicht nicht.
   Vor jedem Hosting prüft `verify` die gesamte Installer-Matrix und Prüfsummen; `downloads` schreibt
   den Index. Erst dann wird das Release angelegt. Fehlende macOS-Aktivierung oder Secrets sind ein
   Fehler, niemals eine unsignierte Ausweichroute oder ein absichtlich unvollständiges Release.
6. Nach dem Release darf nur AUR publizieren (`AUR_ENABLED=true` und Secrets nötig). Bei RCs wird
   **kein AUR-Push** ausgeführt. Homebrew-Tap/Scoop bleiben unveröffentlichte Anschlussmöglichkeiten.

`dist-workspace.toml` verwendet `global-artifacts-jobs` für macOS/native Installer und `publish-jobs`
nur für AUR. Build-Workflows laden eindeutige `artifacts-*` hoch, schreiben selbst kein GitHub Release.
Der Maintainer verändert Secrets/Variablen, veröffentlicht und setzt Tags; Agenten tun das nicht.

### Workflow neu erzeugen

`release.yml` wurde mit dist 0.32.0 regeneriert. Bewusste Handergänzungen am `host`: `custom-ci` in
`needs`, alle Pflichtjobs ausschließlich `success`, `verify`/`downloads` vor Hosting, Index/Manifest
und Downloadtabelle beim Release. `allow-dirty = ["ci"]` schützt diese Ergänzungen. Zum Regenerieren
kurz auskommentieren, `dist generate`, wiederherstellen und **alle** Ergänzungen erneut einsetzen.
Danach `dist plan --tag v0.1.0` und die Offline-Vertragstests ausführen. Keine Actions von Hand starten.

## Plattformspezifische Grenzen

- Linux: Distributionen brauchen glibc >=2.35, libgcc, Fontconfig, xkbcommon einschließlich X11-Helfer,
  Wayland, EGL, Vulkan-Loader sowie X11 mit Cursor-, Input- und XCB-Helfern. Die von winit dynamisch
  geladenen Bibliotheken gehören ausdrücklich zu den Abhängigkeiten, auch wenn `ldd` sie nicht zeigt.
  DEB/RPM/Arch erhalten passende Abhängigkeitsnamen. Kein Snap, keine neue Paketquelle. Arch-ARM ist
  **Arch Linux ARM**, nicht Omarchy. AUR bleibt mit seinen bestehenden PKGBUILD-Vorlagen erhalten.
  RCs sortieren vor Stable: DEB/RPM `0.1.0~rc.1-1`, pacman `0.1.0rc.1-1`.
- Windows: festes `{autopf64}\TorroWorkplace`, Administratorrechte, x64, stabile AppId je App, eigener
  Uninstaller und Startmenüeintrag (Workplace startet `ui`). Gemeinsame Programme, Marker und
  SemVer-Begleitdateien sind 64-Bit-`sharedfile` mit `uninsnosharedfileprompt` und
  `UninstallLogMode=append`. Downgrades oder fremde/beschädigte Dateien blockieren vor dem Kopieren.
  `/DIR` und gespeicherte abweichende Pfade werden abgelehnt. Die Ablehnung im Restart Manager bricht
  vor der Installation ab; nach dessen Schließversuch prüft ein exklusiver Zugriff alle Programme und
  Begleitdateien vor dem ersten Kopieren. Verbleibende Sperren brechen ab, Hash-/Versionsprüfungen
  verhindern das Fortschreiben von Metadaten nach „Skip“. Keine App wird
  erhöht gestartet, kein Systemdienst angelegt und keine persönlichen Daten gelöscht. SmartScreen kann
  wegen fehlender Signatur warnen (Entscheidung X.10). Die Unterstützung der Apps selbst wird durch
  einen Installer nicht erweitert; bekannte Plattformlücken bleiben bestehen.
  Inno fängt Fehler in Dateieintrag-Callbacks ab. Ein Abbruch über das versteckte Wizard-Fenster
  funktionierte im nativen stillen Dienstkontext nicht: Setup meldete trotz Sperre Erfolg und schrieb
  Deinstallationsdaten. Deshalb merken die Prüf-Callbacks Fehler nur vor. Verpflichtende
  Code-Konstanten lösen außerhalb der abgefangenen Callbacks `Abort` aus: am Verzeichnis nach Restart
  Manager und vor Deinstallationsdaten, vor jeder Payload-Zieldatei und an einer letzten externen
  Quelldatei ohne Kopierinhalt. Die letzte Prüfung darf weder durch `Check` noch eine Auswahl entfallen;
  `ExternalSize: 0` verhindert eine vorgezogene Größenabfrage. `/NOCANCEL` und `/LOADINF` bleiben
  abgelehnt. Auch das unveränderte Produktions-Setup lieferte im gezielten nativen Sperrtest Exitcode 5,
  vollständig unveränderte Payload-/Uninstaller-Hashes und unveränderte Deinstallationsdaten der Registry.
  Auch injizierte Ausnahmen bei der Programmprüfung und beim letzten Marker lieferten nativ Exitcode 5,
  ohne neue Uninstaller-Dateien oder Deinstallationsdaten der Registry zu veröffentlichen; bei der
  fehlgeschlagenen Programmprüfung blieb die Versions-Begleitdatei RC. **Kein vollständiger Upgrade-
  Rollback:** Das zuvor kopierte Programm blieb ersetzt; beim letzten Marker blieben fünf Payload-
  Dateien verändert. Eine nach einem späten Fehler beschädigte Installation muss vor Neuinstallation
  entfernt werden. Nach einem durch unerwarteten Windows-Neustart unterbrochenen Gesamtlauf wurde der
  Teilzustand gesichert und über seine Uninstaller entfernt. Die Wiederholung aus leerem Verzeichnis
  bestand vollständig mit Zwischenstandsprotokoll: sieben Apps, RC→Stable, Wiederholungen, gesperrtes
  Upgrade, unsichere INF, Downgrade-Abweisung, beide Entfernungsreihenfolgen und Datenerhalt. Das ist
  ein Nachweis als Gastagenten-SYSTEM, **nicht** für UAC, SmartScreen oder interaktive App-Funktionen.
- macOS: Developer ID Application, Hardened Runtime, Zeitstempel, Notarisierung und Heften für Bündel
  und Image. Keine Universal-Binary; Intel-Runner endet voraussichtlich Herbst 2027. Gleiche
  Service-Bundle-ID und gleiches Team erhalten den Schlüsselbundzugriff; `TorroVersion` enthält RCs,
  Apples Kurzversion nur Zahlen. Entwickler-Bündel bleiben bei `scripts/macos-dev-bundles.sh`.

## Maintainer-Voraussetzungen (keine Änderung durch diese Umsetzung)

- GitHub Actions braucht `contents: write` ausschließlich zum Veröffentlichen durch `host`.
- macOS braucht `MACOS_APP_ENABLED=true` und `MACOS_CERTIFICATE_P12` (Base64 des Developer-ID-P12),
  `MACOS_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_SPECIFIC_PASSWORD`.
  Der frühere Stand dokumentierte deren Einrichtung am 2026-10-11; hier wird kein aktueller Secret-Wert
  gelesen oder geändert. Team bisher `2GA7DQ3P3Z`; Zertifikat vor **2027-02-01** erneuern.
- AUR: Konto, `AUR_SSH_PRIVATE_KEY`, `AUR_USERNAME`, `AUR_EMAIL`, freie Paketnamen und
  `AUR_ENABLED=true`. Bestehende Pakete: sieben `-bin`, `torro-workplace`, `torro-suite`.
- Homebrew später: `mahype/homebrew-torro`, `HOMEBREW_TAP_TOKEN`, `tap`, Publish-Job, Meta-Formel und
  Brewfile. Formeln erzeugt dist aus den Crate-Metadaten (Laufzeitabhängigkeiten mit `stage=["run"]`).
- Scoop später: `mahype/scoop-torro`, vorhandene Manifeste je App; keine Änderung der Archivkonsumenten.

## Prüfprotokoll und offene Nachweise

Offline: `python3 -m unittest discover -s packaging/tests`, `dist plan --tag v0.1.0`,
`scripts/ci-local.sh`, `TORRO_LANG=de cargo test --workspace`. Die Python-Tests prüfen die Matrix,
Begleitpakete, Marker, Versionstransformation, SHA-256, unsichere Archive und Fehlerausgänge ohne Netz.
Ein nFPM-Bau mit kleinen Testprogrammen prüft echte Paketmetadaten, **nicht** den Start echter Apps.

Lokales Protokoll 2026-11-08/09 (OpenCode-Umsetzung, nFPM 2.48.0; erste Paketläufe mit Testprogrammen,
echte Programmprüfungen unten ausdrücklich gekennzeichnet):
Diese Nachweise betreffen die damaligen sieben Apps ohne Chat. Nach Abgleich mit `main` wurde die
Erweiterung um Chat am 13.11.2026 ausdrücklich bestätigt; alte Sieben-App-Ergebnisse sind **kein**
acht-App- oder Chat-Plattformnachweis. Die Hardware-/Desktop-Nachtests laufen als Folge-Issues
[Linux #668](https://github.com/mahype/TorroWorkplace/issues/668) und
[Windows #669](https://github.com/mahype/TorroWorkplace/issues/669), jeweils für die tatsächlich geprüfte Matrix.
Die realen Programmpayloads wurden vor der zusätzlich freigegebenen Client-Sperrkorrektur gebaut;
sie belegen die Installer-/Ladeprüfungen, nicht einen vollständigen Release-Bau des späteren Quellstands.

| Prüfung | Ergebnis / Grenze |
|---|---|
| 0.1.0-rc.1 und 0.1.0, je 48 native Pakete | gebaut, beide Architekturen und alle drei Formate |
| Ubuntu 24.04 arm64, DEB-Smoke | Metadaten, Installieren/Wiederholung, RC→Stable, Entfernen, Daten-/Dienst-Erhalt grün |
| Fedora 43 arm64, RPM-Smoke | dieselben Paketprüfungen grün |
| Arch-Container x86_64, pacman-Smoke | dieselben Paketprüfungen grün; Download-Sandbox nur im wegwerfbaren Docker wegen dessen seccomp-Grenze deaktiviert |
| Inno 7.1.1 unter lokalem CrossOver/Rosetta | alle sieben Setup-Skripte mit Platzhalter-Payload kompiliert; keine Windows-Installationsprüfung. Die echte Versionsabfrage ist `ISCC --version`, nicht die Hilfeausgabe; Regressionstest ergänzt |
| Echte macOS-arm64-Programme, Profil `dist` | alle sieben gebaut; sieben App-Bündel, Reminder und eingebetteter Dienst mit lokaler Developer-ID, Hardened Runtime und Zeitstempel signiert; `codesign --verify --deep --strict` sowie sieben `--version`-Aufrufe grün; nur Systembibliotheken/-Frameworks verlinkt |
| Echte macOS-Intel-Programme, Profil `dist` | alle sieben auf dem M4 quergebaut; Bündel, Reminder und eingebetteter Dienst mit Developer-ID signiert und streng geprüft; sieben `--version`-Aufrufe unter Rosetta grün; kein Nachweis auf Intel-Hardware |
| Echte macOS-Demo-Fenster aus signierten Bündeln | alle sieben arm64- und alle sieben Intel/Rosetta-Bündel starteten mit getrennten Testdaten und öffneten eigene Fenster; Screenshots geprüft; kein Nachweis von Notarisierung, Drag-and-drop-Installation, Begleitfunktionen oder Intel-Hardware |
| Echte Linux-Programme, Ubuntu 22.04 / glibc 2.35 | Stable und RC für arm64 und x64 gebaut; beide vollständigen Matrizen mit je 48 Paketen und finalen GUI-Abhängigkeiten gebaut |
| Echte Linux-Installation/Versionsaufrufe | alle sieben Stable-Programme auf Ubuntu 22.04 arm64/x64, Fedora 43 arm64 und Arch x64 grün; abschließende RC→Stable/Wiederholung/Entfernung samt Datenerhalt mit finalen Abhängigkeiten auf Ubuntu 24.04 arm64, Ubuntu 22.04 x64, Fedora 43 arm64/x64 und Arch x64 grün |
| Linux-X11-Startprüfung im isolierten Xvfb, unprivilegierter Benutzer | fand zusätzlich fehlende dynamische winit-Helfer; nach Ergänzung von Cursor/Input/XCB/xkbcommon-X11 öffneten alle sieben installierten echten arm64-Apps ihre Fenster unter Ubuntu 22.04; kein Nachweis der Begleitfunktionen oder Desktopintegration |
| Echte Windows-x64-Programme und Setups | Stable und RC mit release-identischer statischer C-Laufzeit quergebaut; alle 14 Produktions-Setups mit Code-Konstanten und echtem Inno 7.1.1 erneut gebaut; keine VCRUNTIME/MSVCP/api-ms-win-crt-Importe; zwei Diagnosekopien für späte Fehler getrennt gebaut, niemals Release-Artefakte |
| Windows-VM / echte Apps | offizielles, per Microsoft-Prüfsumme verifiziertes Enterprise-Evaluation-x64-Image installiert; Desktop/Testkonto und reguläre Aktivierung mit knapp 90 Tagen Restlaufzeit; sieben echte Apps als RC und Stable installiert und passende Versionsaufrufe grün; vor Korrektur Sperrfehler nativ reproduziert, nach Korrektur Produktions-Setup im Sperrtest Exitcode 5, null geänderte Datei-Hashes und unveränderte Deinstallations-Registry |
| Native späte Windows-Prüffehler | Ausnahmen in Programmprüfung und letztem Marker nativ injiziert: beide Exitcode 5, Uninstaller-Dateien und Deinstallations-Registry unverändert; Programmfehler schreibt Versions-Begleitdatei nicht fort; bereits ersetzte Payloads werden aber nicht auf RC zurückgesetzt (ein bzw. fünf veränderte Dateien). Diagnose-Installationen danach über ihre Uninstaller entfernt |
| Vollständiger Windows-Lebenszyklus mit Produktions-Setups | **grün**, Exitcode 0: alle sieben Apps, RC→Stable und Wiederholung, `/DIR`/`NOCANCEL`/unsichere INF und gesperrtes Upgrade abgewiesen, Downgrade abgewiesen, beide Entfernungsreihenfolgen, gemeinsame Dateien bis zum letzten Besitzer und persönliche Testdatei erhalten; installierte Programme jeweils mit passender Version geladen. Wiederholung nach gesicherter Bereinigung eines durch unerwarteten OS-Neustart unterbrochenen Laufs; ausführliches lokales Protokoll `target/installer-tools/windows-progress-lifecycle-result.log`; Gastagenten-SYSTEM, keine UAC-/SmartScreen-/GUI-Aussage |
| Lokale CI / Regressionen | neue 16 Python-Verträge für Code-Konstanten vorher rot, nach Korrektur auch unter Python 3.10 grün; vollständige deutsche Workspace-Tests und `scripts/ci-local.sh`-Wiederholung grün. Der erste CI-Lauf scheiterte an zwei Agentenfenster-Tests (`error` statt `unsupported`); beide einzeln sowie gesamte Core-Tests parallel/seriell grün, ohne Codeänderung dort, Ursache weiter ungeklärt. Zuvor ausdrücklich freigegebene Korrekturen für `Protection(Busy)` durch geerbte Dateibeschreibung und EML-`Changed` durch Hintergrund-Sync; danach 60 EML- und 40 Client-Setup-Wiederholungsläufe grün |
| Unabhängiges Nachreview | zwei Runden über lokale Korrekturen und vollständigen Windows-Abbruchpfad; `/LOADINF`-Umgehung gefunden und behoben, zweite Runde ohne weitere Codefunde; kein Ersatz für native Windows-Tests |
| Notarisierte macOS-Images, Gatekeeper | weiterhin offen: Developer-ID lokal verfügbar und verwendet, Notarisierungszugang bisher nur als GitHub-Secrets; kein lokales Profil und kein Release-Workflow gestartet |
| Erstes veröffentlichtes Release | offen, kein Release gestartet |

### Erweiterung um Chat am 13.11.2026

Der aktuelle `main` wurde vor der PR-Vorbereitung übernommen. Die bestätigte Acht-App-Erweiterung steht
als [Umsetzungsergänzung in #589](https://github.com/mahype/TorroWorkplace/issues/589#issuecomment-6851963504).

| Prüfung | Ergebnis / Grenze |
|---|---|
| Echter `dist plan --tag v0.1.0` | exakt acht Programme einschließlich `torrochat`; native Sollmatrix 78 |
| Offline-Verträge | 18 Tests grün: Matrix/Downloads, Chat, Ablehnung fehlender/doppelter/fremder Plan-Einträge, vollständige macOS-Jobmatrix und native Icon-Ressourcen |
| nFPM 2.48.0 / Inno 7.1.1 | alle 54 Linux-Pakete und acht Windows-Setups mit **Platzhalter-Payloads** gebaut; Paketierungs-/Compiler-Nachweis, kein neuer App-Laufzeitnachweis |
| Chat für Windows | `cargo xwin check --locked -p torrochat --target x86_64-pc-windows-msvc` mit statischer CRT grün, einschließlich neuem Icon-/Versions-Buildskript; kein nativer GUI- oder Installationsnachweis |
| Vollständige lokale CI / Deutsch | `scripts/ci-local.sh` und `TORRO_LANG=de cargo test --workspace` nach korrigierten Demo-Zählannahmen grün; 18 Offline-Verträge zusätzlich unter tatsächlichem Python 3.10 grün |
| Katalog-Demo / Sichtprüfung | Regression deckt exakt `AppId::ALL` einschließlich Chat ab; unabhängiger Reviewer hat alle zwölf frischen PNGs (hell/dunkel, sechs Größen) und das Chat-Icon gelesen: keine neuen Muss-Funde. Mini/Leiste zeigen die bestehende Workplace-Übersicht; kein Desktop-/UAC-/Gatekeeper-Nachweis |
| API-/Katalogschutz | unabhängige statische Prüfung ohne Blocker: bestehende Request-/MCP-Wege und Rechte, native Marker, Preflight aller Ziele/Abhängigkeiten und Setup-Cleanup-Sperre. Zusätzlicher gezielter Cleanup-Regressionstest wurde als nichtblockierende Testanregung genannt |

Die alten Sieben-App-Laufzeitnachweise bleiben oben unverändert. Neue Hardware-/Desktop-Prüfungen
einschließlich Chat sind in #668/#669 offen; macOS-Notarisierung, Gatekeeper und der veröffentlichte
Release-Nachweis bleiben ebenfalls offen. Keinen lokalen Testbau als veröffentlichtes Release ausweisen.

`packaging/tests/linux-smoke.sh` prüft in einem **wegwerfbaren Container** Metadaten, Installieren,
Wiederholung, Upgrade RC→Stable und Entfernen mit verbleibendem Dienst und erhaltenen Daten.
`windows-smoke.ps1` läuft nur in einer **frischen Windows-x64-VM** mit Administratorrechten und
`TORRO_INSTALLER_SMOKE=1`: alle acht Apps, Wiederholung, Upgrade, Downgrade-Abweisung, beide
Entfernungsreihenfolgen, gemeinsame Dateien bis zum letzten Besitzer; installierte echte Programme müssen
mit `--version` laden und exakt zur Versions-Begleitdatei passen (30-Sekunden-Grenze). Nie am
Arbeitsrechner ausführen. Der lokale Windows-Testbau muss wie dist 0.32.0 mit
`-C target-feature=+crt-static` erfolgen (`msvc-crt-static` ist dort standardmäßig `true`), sonst prüft er
eine andere C-Laufzeitabhängigkeit als die Release-Binaries.

Vor Freigabe je Ziel/Architektur festhalten: Version/OS, Befehl, Resultat, Prüfer und Datum. Weiter von
Hand zu prüfen: jede App starten, Partner-Apps/Verfassen/Erinnerung, Installieren/Entfernen und erhaltene
Daten; macOS Drag-and-drop/Gatekeeper beider Architekturen; Windows UAC/SmartScreen und interaktive
Ablehnung des Schließens laufender Apps; reale Downloadmatrix durch Maintainer. Ohne diese Nachweise bleibt #589 offen,
PR nur **`Related to #589`**, kein `Closes`.

Technische Referenzen: [dist 0.32.0](https://github.com/axodotdev/cargo-dist/tree/v0.32.0),
[nFPM](https://nfpm.goreleaser.com/docs/configuration/),
[Inno gemeinsame Dateien](https://jrsoftware.org/ishelp/topic_filessection.htm#sharedfile).
