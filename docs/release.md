# Release-Pipeline

Stand: 2026-11-08, #589. **Noch kein veröffentlichter Installer-Nachweis.** Der Code bereitet die
Downloads vor; erst ein Maintainer-Release mit Plattformprüfungen belegt die vollständige Auslieferung.
Anleitung für Nutzer: [install.md](install.md). Maintainer-Ablauf: [release-0.1.0.md](release-0.1.0.md).

## Artefakte und Begleitpakete

`packaging/apps.json` nennt die sieben stabilen App-IDs, Programme, Startargumente und Abhängigkeiten:
Workplace, Notification Hub, Workplace Mail, App Feed, Calendar, Tasks und Assistant. Bundle-ID bleibt
`io.github.mahype.<app-id>`, die des Dienstes `io.github.mahype.torro-workplace-service`.

| Plattform | Installierbarer Download | Inhalt |
|---|---|---|
| macOS arm64 / Intel | `<bin>-<version>-<target>.dmg` | signierte, notarisierte, geheftete App; Mail/App Feed zusätzlich Hub, Calendar zusätzlich Reminder; jede App mit eingebettetem Dienst |
| Windows x64 | `<bin>-<version>-x86_64-pc-windows-msvc-setup.exe` | unsignierter Inno-Setup-Installer für alle Benutzer, App, Dienst, bei Mail/App Feed Hub |
| Linux amd64 / arm64 | native nFPM-Namen, `.deb`, `.rpm`, `.pkg.tar.zst` | acht Pakete je Ziel und Format: Dienst, Workplace-Starter und sechs Apps |

Linux besitzt `torroworkplace-bin` allein das Programm `/usr/bin/torroworkplace` und
`/usr/lib/systemd/user/torroworkplace.service`. `torro-workplace` besitzt nur Starter und Icon. Die sechs
`<bin>-bin`-Pakete besitzen jeweils Programm, Paketmarker, Starter und Icon. Jedes App-Paket braucht den
Dienst mindestens derselben nativen Version; Mail/App Feed auch `torroinbox-bin`. Kein doppelter
Dateibesitz, keine Root-Dienststarts, kein automatisches Aktivieren der Nutzereinheit oder Autostart.

Die Pipeline verlangt **69 Installer**: 48 Linux-Pakete, 14 DMGs und sieben Setups. Dazu gehören je Datei
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

1. `dist` **0.32.0** plant alle sieben Programme für fünf Ziele. `custom-ci` prüft Linux, macOS und Windows.
2. `build-local-artifacts` baut auf Ubuntu 22.04 (x86_64/arm64), macOS 14 (arm64), macOS 15 Intel und
   Windows 2022 die Archive. Linux-Basis: glibc **>= 2.35**, dynamische Grafikloader für X11/Wayland.
3. `build-global-artifacts` erzeugt die bisherigen Skriptinstaller, Formeln und Gesamtprüfsummen.
4. Globale Artefakt-Jobs (auch bei RCs):
   - `publish-macos-app.yml::macos-app` lädt **Workflow-Artefakte**, prüft Tag und SHA-256, baut die
     Bündel mit `make-app.sh`, signiert innen nach außen, notarisiert und heftet. Ein Folgejob baut mit
     `make-dmg.sh` sieben Images je Architektur: Applications-Link, englische Ziehanleitung, benötigte
     Partner-Bündel. Auch DMGs werden signiert, notarisiert, geheftet und validiert. Temporäre Mounts
     werden aufgeräumt. App-Archivnamen, Dienst und Bundle-IDs bleiben unverändert.
   - `build-installers.yml` nutzt nFPM **2.48.0** und Inno Setup **7.1.1**, deren Download-SHA-256 im
     Workflow feststeht. `installers.py` prüft die sieben Quellen im dist-Plan, Tag und Quellprüfsummen,
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

- Linux: Distributionen brauchen glibc >=2.35, libgcc, xkbcommon, Wayland, EGL, Vulkan-Loader und X11.
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
  Inno fängt Fehler in Dateieintrag-Callbacks ab: Die Schutzprüfungen setzen deshalb den nativen
  Abbruch über `WizardForm.Close` und sperren nach einem Fehler weitere Payload-Einträge. Die
  Sperrprüfung liegt an einem Verzeichniseintrag nach Restart Manager und vor den Dateikopien;
  direkt danach verarbeitet Inno den Abbruch. `/NOCANCEL` wird abgelehnt, auch stille Installationen
  müssen sicher abbrechen können. Dieser Ablauf ist am gepinnten Inno-Quelltext statisch geprüft;
  der echte Windows-VM-Nachweis steht weiter aus.
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

Lokales Protokoll 2026-11-08 (OpenCode-Umsetzung, nFPM 2.48.0, Testprogramme statt Release-Binaries):

| Prüfung | Ergebnis / Grenze |
|---|---|
| 0.1.0-rc.1 und 0.1.0, je 48 native Pakete | gebaut, beide Architekturen und alle drei Formate |
| Ubuntu 24.04 arm64, DEB-Smoke | Metadaten, Installieren/Wiederholung, RC→Stable, Entfernen, Daten-/Dienst-Erhalt grün |
| Fedora 43 arm64, RPM-Smoke | dieselben Paketprüfungen grün |
| Arch-Container x86_64, pacman-Smoke | dieselben Paketprüfungen grün; Download-Sandbox nur im wegwerfbaren Docker wegen dessen seccomp-Grenze deaktiviert |
| Windows-VM / echte Apps | noch nicht ausgeführt; Wine ersetzt diesen Nachweis nicht |
| Signierte/notarisierte macOS-Images, Gatekeeper | ohne Maintainer-Signierung nicht ausgeführt |
| Erstes veröffentlichtes Release | offen, kein Release gestartet |

`packaging/tests/linux-smoke.sh` prüft in einem **wegwerfbaren Container** Metadaten, Installieren,
Wiederholung, Upgrade RC→Stable und Entfernen mit verbleibendem Dienst und erhaltenen Daten.
`windows-smoke.ps1` läuft nur in einer **frischen Windows-x64-VM** mit Administratorrechten und
`TORRO_INSTALLER_SMOKE=1`: alle sieben Apps, Wiederholung, Upgrade, Downgrade-Abweisung, beide
Entfernungsreihenfolgen, gemeinsame Dateien bis zum letzten Besitzer. Nie am Arbeitsrechner ausführen.

Vor Freigabe je Ziel/Architektur festhalten: Version/OS, Befehl, Resultat, Prüfer und Datum. Weiter von
Hand zu prüfen: jede App starten, Partner-Apps/Verfassen/Erinnerung, Installieren/Entfernen und erhaltene
Daten; macOS Drag-and-drop/Gatekeeper beider Architekturen; Windows UAC/SmartScreen/belegte Dateien
einschließlich Abbruch; reale Downloadmatrix durch Maintainer. Ohne diese Nachweise bleibt #589 offen,
PR nur **`Related to #589`**, kein `Closes`.

Technische Referenzen: [dist 0.32.0](https://github.com/axodotdev/cargo-dist/tree/v0.32.0),
[nFPM](https://nfpm.goreleaser.com/docs/configuration/),
[Inno gemeinsame Dateien](https://jrsoftware.org/ishelp/topic_filessection.htm#sharedfile).
