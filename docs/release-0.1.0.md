# Anleitung: Version 0.1.0 veröffentlichen

Stand: 2026-10-11. Alles ist vorbereitet (Version 0.1.0, Release-Workflow, signierte macOS-Apps). Was noch
fehlt, braucht deine Zugangsdaten oder deine Entscheidung. Hintergrund und Einzelheiten: [release.md](release.md).

## Was das Release enthält

Für jede App (`torroworkplace`, `torrocalendar`, `torrotasks`):

| System | Datei | Hinweis |
|---|---|---|
| Linux x86_64 / arm64 | `<app>-x86_64-unknown-linux-gnu.tar.xz`, `<app>-aarch64-unknown-linux-gnu.tar.xz` | dazu `<app>-installer.sh` |
| Windows x86_64 | `<app>-x86_64-pc-windows-msvc.zip` | dazu `<app>-installer.ps1`; unsigniert, SmartScreen warnt beim ersten Start |
| macOS Apple Silicon / Intel | `<app>-aarch64-apple-darwin.tar.xz`, `<app>-x86_64-apple-darwin.tar.xz` | Kommandozeile |
| macOS Apple Silicon / Intel | `<app>-aarch64-apple-darwin.app.zip`, `<app>-x86_64-apple-darwin.app.zip` | signiert und notarisiert, zum Doppelklicken (`Torro Workplace.app`, `Torro Calendar.app`, `Torro Tasks.app`) – erst nach Schritt 1 und 2 |

Prüfsummen (`*.sha256`, `sha256.sum`) und `source.tar.gz` kommen dazu.

Bekannt für 0.1.0: Das Tray-Icon gibt es bisher nur unter Linux; auf macOS und Windows läuft der Dienst
ohne Icon im Hintergrund. Die Apps haben noch das Standard-Icon des Systems.

## Schritt 1: Secrets für die macOS-Apps eintragen

Dieselben Werte wie bei TorroMail, nur in diesem Repository noch einmal. In einem Terminal auf dem Mac,
auf dem das Zertifikat liegt. `gh` fragt bei jedem Befehl nach dem Wert, er landet nicht in der Shell-History.

1. Zertifikat exportieren (falls die `.p12` nicht mehr da ist): Schlüsselbundverwaltung → „Developer ID
   Application: … (TEAMID)“ → Rechtsklick → Exportieren → Format „Persönliche Informationsaustauschdatei
   (.p12)“ → Passwort vergeben.
2. Secrets setzen:

```bash
base64 -i DeveloperID.p12 | gh secret set MACOS_CERTIFICATE_P12 --repo mahype/TorroWorkplace
```
```bash
gh secret set MACOS_CERTIFICATE_PASSWORD --repo mahype/TorroWorkplace
```
```bash
gh secret set APPLE_ID --repo mahype/TorroWorkplace
```
```bash
gh secret set APPLE_TEAM_ID --repo mahype/TorroWorkplace
```
```bash
gh secret set APPLE_APP_SPECIFIC_PASSWORD --repo mahype/TorroWorkplace
```

Das app-spezifische Passwort gibt es unter appleid.apple.com → Anmeldung und Sicherheit →
App-spezifische Passwörter (ein neues für „TorroWorkplace notarytool“ ist am saubersten).

Kontrolle: `gh secret list --repo mahype/TorroWorkplace` zeigt die fünf Namen.

## Schritt 2: macOS-Apps einschalten

```bash
gh variable set MACOS_APP_ENABLED --body true --repo mahype/TorroWorkplace
```

Ohne diese Variable überspringt das Release die macOS-Apps (alles andere entsteht trotzdem).

## Schritt 3: Release-Branch nach `main`

Der Pull Request „Release 0.1.0“ (`release/0.1.0`) muss grün sein, vor allem „clippy + test“ auf macOS
und Windows, dann mergen.

## Schritt 4: Tag setzen

Auf dem aktuellen `main`:

```bash
git switch main && git pull --ff-only && git tag v0.1.0 && git push origin v0.1.0
```

Das startet „Release“ in GitHub Actions (etwa 15–25 Minuten): Builds für alle Systeme, GitHub Release
`v0.1.0`, danach die macOS-Apps (Signieren und Notarisieren dauert je App einige Minuten).

## Schritt 5: Prüfen

- Release-Seite: `https://github.com/mahype/TorroWorkplace/releases/tag/v0.1.0` – je App 5 Archive,
  2 Installer, bei eingeschaltetem macOS-Job 2 `.app.zip`.
- macOS: `.app.zip` herunterladen, entpacken, Doppelklick – startet ohne Gatekeeper-Warnung.
  Kontrolle im Terminal: `spctl --assess --type execute --verbose "Torro Calendar.app"` → „accepted,
  source=Notarized Developer ID“.
- Windows: `.zip` entpacken, `torrocalendar.exe` starten; SmartScreen „Trotzdem ausführen“.
- Linux: `tar -xf torrocalendar-x86_64-unknown-linux-gnu.tar.xz && ./torrocalendar-x86_64-unknown-linux-gnu/torrocalendar`.

## Wenn etwas schiefgeht

- **Build scheitert:** Fehler beheben, dann Tag und (falls angelegt) Release löschen und neu setzen:
  `gh release delete v0.1.0 --yes --cleanup-tag --repo mahype/TorroWorkplace`, danach Schritt 4.
- **Nur die macOS-Apps scheitern** (z. B. falsches Passwort): Das Release mit allen anderen Dateien steht
  schon. Secrets korrigieren, dann die Apps nachliefern: Actions → „publish-macos-app“ → „Run workflow“
  mit Tag `v0.1.0`.
- **Notarisierung abgelehnt:** Im Log steht eine Submission-ID; Details mit
  `xcrun notarytool log <ID> --apple-id … --team-id … --password …`.

## Später

Homebrew-Tap, AUR, Scoop, Windows-Signatur, echte App-Icons: Checkliste in [release.md](release.md),
Abschnitt „Einmal einrichten“.
