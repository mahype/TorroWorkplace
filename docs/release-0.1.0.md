# Anleitung: Version 0.1.0 veröffentlichen

Stand: 2026-11-13. **Nur für Maintainer. Kein veröffentlichter Release-Nachweis.** Agenten starten
keinen Release, setzen keine Tags und ändern keine Secrets. Details: [release.md](release.md).

## Vor dem Tag

1. Gemeinsame Workspace-Version und Lockfile müssen zur Version passen; Notizen aktualisieren.
2. Lokal `python3 -m unittest discover -s packaging/tests`, `dist plan --tag v0.1.0`,
   `scripts/ci-local.sh` und `TORRO_LANG=de cargo test --workspace` grün ausführen.
3. macOS-Voraussetzungen prüfen: `MACOS_APP_ENABLED=true`, gültiges Developer-ID-Zertifikat und die
   fünf Secrets aus [release.md](release.md). Ohne sie bricht das **gesamte** Release ab.
4. Erst RC veröffentlichen und Installationen in frischen Systemen prüfen; dafür muss auch die
   Workspace-Version `0.1.0-rc.N` lauten. RC baut alle Installer, aktualisiert AUR nicht.
5. Smoke-Tests nur in wegwerfbaren Testsystemen: Linux `bash packaging/tests/linux-smoke.sh
   <deb|rpm|archlinux> <rc-verzeichnis> <stable-verzeichnis>` mit `TORRO_INSTALLER_SMOKE=1` im Container;
   Windows `packaging/tests/windows-smoke.ps1 -Older <RC> -Newer <Stable>` in frischer Admin-VM.

## Veröffentlichen

Nach Review/Merge und grünen lokalen Prüfungen setzt und pusht **der Maintainer** den Versions-Tag.
Nur dieses Tag-Ereignis startet den Release-Workflow. Keine manuelle Ausführung eines Installer-Jobs.
Vor `host` müssen CI, alle Archive und **alle** Installer erfolgreich sein. Bei fehlendem macOS-Secret,
Toolfehler oder fehlendem Installer gibt es keinen absichtlich unvollständigen Release.

## Downloads prüfen

- [Release-Seite](https://github.com/mahype/TorroWorkplace/releases/tag/v0.1.0): `downloads.md` und
  `installers-manifest.json`, 78 Installer und 78 zugehörige `.sha256` vorhanden und eindeutig zugeordnet.
- Linux: je Architektur neun DEB, neun RPM, neun pacman-Pakete. Die Downloadtabelle verlinkt die
  notwendigen Begleitpakete desselben Formats und derselben Architektur.
- Windows: acht `<bin>-0.1.0-x86_64-pc-windows-msvc-setup.exe`, systemweit, unsigniert.
- macOS: 16 `<bin>-0.1.0-<target>.dmg`, dazu weiterhin 18 `.app.zip` einschließlich Reminder.
- Bisherige Programmarchive, Skriptinstaller, Formeln, Lizenzen und Prüfsummen bleiben vorhanden.

Je App/System/Architektur: Installieren, Start, Upgrade, Entfernen, erhaltene persönliche Daten und
verbleibende Partner-Apps protokollieren. macOS: alle angezeigten Bündel nach Applications ziehen,
Image auswerfen, ohne Gatekeeper-Warnung starten; Mail-Verfassen und Calendar-Erinnerung testen.
Windows: UAC, SmartScreen, Schließen belegter Apps/Dienst, Ablehnung ohne Teiländerung, beide
Entfernungsreihenfolgen gemeinsamer Komponenten testen. Nutzeranleitung: [install.md](install.md).

## Fehler und Abschluss

Fehler vor `host`: Log lesen und Ursache beheben; ein gescheiterter macOS-Job liefert nichts nachträglich
an ein bereits veröffentlichtes Teilrelease. Keine Secrets im Log ausgeben. Notarisierungsdetails nur
mit Submission-ID über `notarytool log` prüfen. Ein erneuter Release-Lauf bleibt eine
Maintainer-Entscheidung; Tags/Veröffentlichungen niemals automatisch löschen oder neu setzen.

Erst nach dokumentierter vollständiger Downloadmatrix und Plattformprüfungen den Release-Nachweis an
#589 ergänzen und über das Schließen entscheiden. Bis dahin bleiben entsprechende Erledigt-Punkte
offen; der Implementierungs-PR verwendet `Related to #589`.
