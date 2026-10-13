# Release-Pipeline

Stand: 2026-10-11. Entwurf (Aufgabe K.16). Nichts davon ist veröffentlicht: kein Tag, keine Repositories,
keine Secrets. Alles, was ein Secret braucht, ist abgeschaltet, bis die Checkliste unten abgehakt ist.

**Schritt für Schritt für 0.1.0:** [release-0.1.0.md](release-0.1.0.md).

Ziel: `git tag v0.1.0 && git push --tags` baut die ganze Suite (`torroworkplace`, `torrocalendar`,
`torrotasks`) für Linux, macOS und Windows, legt sie in ein GitHub Release und aktualisiert die
Paketquellen.

## Empfehlung

`dist` (früher cargo-dist) für das Release, dazu zwei kleine eigene Workflows für das, was `dist` nicht kann.

| Baustein | Aufgabe | Wer |
|---|---|---|
| `dist-workspace.toml` + `.github/workflows/release.yml` | Binärarchive für 5 Ziele, Shell-/PowerShell-Installer, Homebrew-Formeln, GitHub Release | `dist` (Workflow ist generiert, nicht von Hand ändern) |
| `.github/workflows/publish-aur.yml` | AUR-Pakete `-bin` und `torro-suite` aktualisieren | automatisch nach dem Release („custom publish job“) oder von Hand |
| `.github/workflows/publish-macos-app.yml` | signierte, notarisierte `.app`-Bündel für die GUIs | automatisch nach dem Release („custom publish job“) oder von Hand |
| `packaging/` | `.desktop`-Dateien, Icons, PKGBUILD-Vorlagen, Meta-Formel, Brewfile, Scoop-Vorlage | von Hand |

Gründe:

- `dist` 0.33.0 (2026-09-10) ist aktiv gepflegt: Releases im Februar, Mai und September 2026.
  Auf crates.io steht nur 0.32.0; die Konfiguration ist mit 0.32.0 erzeugt und lokal geprüft
  (`dist plan`, `dist build` für `x86_64-unknown-linux-gnu`, alle drei Archive entstehen).
- Ein Tag `v0.1.0` (ohne Paketnamen) veröffentlicht alle Pakete des Workspaces mit dieser Version in
  einem Release. Das passt, weil die Suite eine gemeinsame Version hat (`[workspace.package]`).
  Reine Bibliotheks-Crates (`torro-*`) ignoriert `dist`.
- `dist` erzeugt pro App eine Homebrew-Formel und pusht sie in den Tap. Die Formeln stehen heute schon
  in der Plan-Ausgabe (`torrocalendar.rb` usw.).
- Das Gerüst ist ohne Secrets lauffähig: Archive, Installer und Release brauchen nur `GITHUB_TOKEN`.
  Secrets kommen erst bei Tap, AUR, Signierung dazu.
- Was `dist` nicht kann, bleibt klein und austauschbar: `.app`-Bündel (dist kennt keine, siehe unten)
  und AUR. Beides hängt als eigener Job am Release und blockiert es nicht.

### Alternativen

| Alternative | Urteil |
|---|---|
| Eigene Actions-Matrix + `softprops/action-gh-release` | Mehr Handarbeit (Archive, Prüfsummen, Installer, Formeln selbst). Erst sinnvoll, wenn `dist` nicht mehr gepflegt wird. |
| `cargo-packager` (CrabNebula) | Kann `.app`, `.dmg`, `.deb`, AppImage, NSIS, MSI. README beschreibt es nicht als stabil; Notarisierung dort nicht dokumentiert. Später prüfen, wenn echte Installer (dmg, MSI) gewünscht sind. |
| `cargo-bundle` | Nur Bündel pro Crate, keine Notarisierung, kein Release-Ablauf. Unser `make-app.sh` (30 Zeilen) reicht. |
| `dist` mit `msi`/`pkg`-Installer | MSI braucht WiX v3 und baut je App ein eigenes MSI; `pkg` installiert nur nach `/usr/local`, keine `.app`. Vorerst nicht. |

## Ablauf eines Releases

| Schritt | Wer | Was |
|---|---|---|
| 1 | Maintainer | `version` in `[workspace.package]` der Wurzel-`Cargo.toml` erhöhen (heute `0.0.1`), `cargo check` (aktualisiert `Cargo.lock`), CHANGELOG/Notizen nachziehen. |
| 2 | Maintainer | `dist plan --tag v0.1.0` zeigt, was gebaut wird. Muss ohne Fehler laufen; ein Tag, der nicht zur Version passt, bricht ab. |
| 3 | Maintainer | Commit nach `main` (per PR, CI grün), dann `git tag v0.1.0 && git push origin v0.1.0`. Ein Tag mit Suffix (`v0.1.0-rc.1`) wird als Prerelease markiert und aktualisiert Tap und AUR nicht. |
| 4 | CI `plan` | Liest den Tag, entscheidet, welche Apps und Ziele gebaut werden. |
| 5 | CI `build-local-artifacts` | Baut je Ziel `.tar.xz` (Windows `.zip`) mit Binary, Lizenzen, README: macOS arm64 (`macos-14`), macOS x86_64 (`macos-15-intel`), Linux x86_64 (`ubuntu-22.04`), Linux arm64 (`ubuntu-22.04-arm`), Windows (`windows-2022`). |
| 6 | CI `build-global-artifacts` | Erzeugt Shell-/PowerShell-Installer je App, Homebrew-Formeln, `sha256.sum`, `source.tar.gz`. |
| 7 | CI `host` | Legt das GitHub Release an und lädt alle Dateien hoch. |
| 8 | CI Publish-Jobs | macOS-`.app.zip` (signiert, notarisiert) ans Release. Noch abgeschaltet: Homebrew-Tap, AUR. |
| 9 | CI `announce` | Schließt den Lauf ab. |

Wo die Artefakte landen: GitHub Release `v0.1.0` im Repository (Archive, Installer, Formeln, Prüfsummen);
Tap `mahype/homebrew-torro` (`Formula/*.rb`); AUR-Repositories `torroworkplace-bin`, `torrocalendar-bin`,
`torrotasks-bin`, `torro-suite`.

Pull Requests laufen nur bis `plan` (`pr-run-mode = "plan"`), das ist billig und fängt eine kaputte
`dist`-Konfiguration früh.

## Pro Betriebssystem

| System | Ziele | Format | Signierung | Installieren |
|---|---|---|---|---|
| Linux | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | `.tar.xz`; AUR-Pakete | nicht nötig; Prüfsummen im Release | Arch: `yay -S torro-suite`; sonst: Shell-Installer je App oder Tarball |
| macOS | `aarch64-apple-darwin`, `x86_64-apple-darwin` | `.tar.xz` mit Binary; später `.app.zip` für die GUIs | Binary: optional. `.app`: Developer ID + Notarisierung nötig, damit ein Doppelklick nach Download nicht blockiert | `brew install mahype/torro/torro-suite` |
| Windows | `x86_64-pc-windows-msvc` | `.zip`, PowerShell-Installer | optional; ohne Signatur zeigt SmartScreen eine Warnung | `powershell -c "irm <URL>/torroworkplace-installer.ps1 \| iex"`; später `scoop install` |

Der Shell-Installer legt je App eine Datei in `~/.cargo/bin` (`install-path = "CARGO_HOME"`). Die ganze
Suite heißt also drei Aufrufe oder das geplante `install.sh` (siehe `docs/research.md`, Abschnitt 7).

### Linux

- Gnu statt musl, bewusst: Die Iced-Apps laden zur Laufzeit per `dlopen` `libwayland-client`,
  `libxkbcommon`, `libX11`, `libvulkan` und `libEGL` (geprüft: `ldd` zeigt nur `libc`, `libm`, `libgcc_s`; die
  übrigen Namen stehen als Zeichenketten im Binary). Ein statisches musl-Binary kann das nicht.
  Der Nutzer braucht die Grafik-Bibliotheken seiner Distribution, sonst nichts.
- Der Build braucht keine Systembibliotheken (`Cargo.lock` enthält weder `libdbus-sys` noch `openssl-sys`;
  Tray und Benachrichtigungen laufen über `zbus`, TLS über `rustls`).
- Die Linux-Binaries entstehen auf Ubuntu 22.04 (glibc 2.35). Ältere Distributionen laufen damit nicht.
- Startmenü: `packaging/linux/torro-calendar.desktop`, `torro-tasks.desktop`, `torro-workplace.desktop`.
  `StartupWMClass` entspricht der App-ID aus `torro-core` (`AppId::window_class`), damit Hyprland-Regeln
  und Launcher das Fenster zuordnen. Die Icons sind **Platzhalter** (`packaging/linux/icons/*.svg`).
- systemd-Nutzereinheit für den Kern: `packaging/linux/torroworkplace.service` (`/usr/bin/torroworkplace daemon`).
- AUR: `torroworkplace-bin`, `torrocalendar-bin`, `torrotasks-bin` (je `x86_64` und `aarch64`, Quelle ist das
  Release-Archiv, Prüfsummen setzt `updpkgsums`) und das Metapaket `torro-suite` (`arch=any`, nur `depends`).
  Vorlagen: `packaging/aur/PKGBUILD.in`, `PKGBUILD.suite.in`. Der Job nutzt
  `KSXGitHub/github-actions-deploy-aur` (Eingaben `pkgname`, `pkgbuild`, `updpkgsums`, `test`,
  `ssh_private_key`, `commit_username`, `commit_email`).

### macOS

- `dist` baut nur nackte Binaries und kennt weder `.app`-Bündel noch Notarisierung (im Quelltext von 0.32.0:
  „in the future, this module will also support notarization“). Signieren kann es mit `macos-sign = true`
  und den Secrets `CODESIGN_IDENTITY`, `CODESIGN_CERTIFICATE`, `CODESIGN_CERTIFICATE_PASSWORD`. Das ist für
  die Binaries nicht eingeschaltet.
- Warum die nackten Binaries reichen: Was per `curl | sh` oder `brew` kommt, trägt kein Quarantäne-Attribut,
  Gatekeeper prüft es nicht. Das gilt für den Kern und für Start aus dem Terminal. Ein Doppelklick auf eine
  heruntergeladene GUI-App ohne Notarisierung wird blockiert.
- Für die GUIs gibt es deshalb `publish-macos-app.yml`: lädt das fertige Binary aus dem
  Release, `packaging/macos/make-app.sh` baut `Torro Calendar.app` (Info.plist aus `Info.plist.in`, optional
  `.icns` aus einem 1024-px-PNG), dann `codesign --options runtime`, `xcrun notarytool submit --wait`,
  `xcrun stapler staple`, `ditto` zu `<bin>-<arch>-apple-darwin.app.zip`, Upload ans Release.
- Nötig dafür: Apple Developer Program (99 USD/Jahr), Zertifikat „Developer ID Application: Sven Wagener
  (2GA7DQ3P3Z)“, für die Notarisierung Apple-ID, Team-ID und ein app-spezifisches Passwort (wie TorroMail).
  Die Signier-Identität liest der Workflow aus dem importierten Zertifikat. `notarytool` nimmt `.zip`, `.dmg`
  und `.pkg`; eine `.zip` lässt sich nicht heften, daher wird das Bündel geheftet und danach gezippt. Zum
  Schluss prüft `spctl --assess`, dass Gatekeeper die App annimmt.
- Das Zertifikat läuft am **2027-02-01** ab. Bereits signierte, notarisierte Apps bleiben gültig (Zeitstempel);
  für neue Releases vorher im Developer-Portal ein neues „Developer ID Application“ erzeugen, als `.p12`
  exportieren und `MACOS_CERTIFICATE_P12` und `MACOS_CERTIFICATE_PASSWORD` hier und bei TorroMail ersetzen.
- Keine Universal-Binary: `dist` kann `universal2-apple-darwin` (noch) nicht als Ziel. Es gibt zwei Archive.
  GitHub stellt den letzten Intel-Runner (`macos-15-intel`) bis Herbst 2027 bereit; danach entfällt
  `x86_64-apple-darwin` oder muss gecrosscompiled werden.
- **Schlüsselbund** (X.13): Alle Konto-Geheimnisse liegen auf macOS in einem Eintrag (Dienst `torroworkplace`,
  Konto `torro-vault`), den der Dienst einmal je Start liest. macOS fragt also höchstens einmal; mit „Immer
  erlauben“ gar nicht mehr, solange die Signatur gleich bleibt. Einträge von früher (ein Eintrag je Konto) zieht
  der Dienst beim ersten Lesen um.
- **Entwicklungs-Builds** (X.14): Debug-Builds sind ad hoc signiert, mit einer Kennung, die sich mit jedem Build
  ändert; der Schlüsselbund erkennt sie danach nicht wieder. Einmal `scripts/macos-dev-cert.sh` ausführen (legt
  das Zertifikat „Torro Development“ an und vertraut ihm für Codesignatur, fragt nach dem Passwort). Danach
  signiert `cargo run` über den Runner in `.cargo/config.toml` jedes Programm mit diesem Zertifikat und der
  Kennung `io.github.mahype.dev.<name>`; wer die Binaries anders startet, signiert sie mit
  `scripts/macos-dev-sign.sh`. Ohne das Zertifikat (CI, andere Rechner) tun beide nichts.
- Homebrew: `dist` erzeugt nur Formeln (kein Cask). Ein Cask für die `.app.zip` kann später von Hand in
  `Casks/` des Taps liegen; Versions- und Prüfsummen-Update müsste ein eigener Schritt übernehmen.

### Windows

- Archiv `.zip` pro App und PowerShell-Installer. Kein MSI: bräuchte WiX v3 und gäbe drei getrennte MSIs.
- Ohne Signatur zeigt SmartScreen beim Browser-Download eine Warnung. `dist` 0.33.0 kann mit Azure Artifact
  Signing signieren (nur x86_64; Secrets `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`, OIDC,
  GitHub-Environment `release`); früher nur SSL.com eSigner. Für 0.1.0 nicht eingeschaltet.
- Der Tray-Dienst ist unter Windows noch nicht implementiert (`tray/` kennt nur Linux). Das Release baut
  trotzdem, die CI ist auf allen drei Systemen grün.
- Scoop später: ein Bucket `mahype/scoop-torro`, je App ein Manifest mit `checkver`/`autoupdate`
  (Vorlage `packaging/scoop/`). Ein Manifest beschreibt genau eine App; ein Metapaket gibt es nicht, also
  `scoop install torroworkplace torrocalendar torrotasks`.

### Homebrew-Tap

- Repository `mahype/homebrew-torro` (Name muss mit `homebrew-` beginnen). `dist` schreibt
  `Formula/torroworkplace.rb`, `torrocalendar.rb`, `torrotasks.rb` bei jedem Release.
- Die Suite als Ganzes: Meta-Formel `torro-suite` (`packaging/homebrew/torro-suite.rb`, einmal von Hand in den
  Tap legen) oder das Brewfile (`brew bundle --file=packaging/homebrew/Brewfile`).
- Formeln sind nur für fertige Archive möglich (kein Bauen aus dem Quelltext). Linux-Nutzer von Homebrew
  bekommen dieselben Formeln.
- Laut Homebrew-Dokumentation müssen Drittanbieter-Taps vertraut werden (`brew trust --tap mahype/torro`).
  Das gehört in die Installationsanleitung, sobald es so verlangt wird.

## Einmal einrichten (Checkliste für den Maintainer)

Reihenfolge ist egal, außer wo vermerkt. Erst wenn ein Punkt erledigt ist, die genannte Zeile aktivieren.

### Ohne Konto-Kosten, sofort möglich

- [ ] Tag `v0.0.x`-Probelauf auf einem Fork oder mit `v0.1.0-rc.1`: Der erste Lauf deckt Fehler im Build auf.
      Das Release dort danach löschen.
- [ ] Im Repository: Settings → Actions → „Workflow permissions“ auf Lesen und Schreiben (Release anlegen).
- [ ] Eine `dist`-Version ≥ 0.33 prüfen: `dist init` aktualisiert Konfiguration und Workflow. Vorsicht:
      0.33 liegt nicht auf crates.io, sondern nur als GitHub-Release.
- [ ] `release.yml` nicht von Hand ändern, immer `dist generate` nach Änderungen an `dist-workspace.toml`.

### Homebrew-Tap

- [ ] Repository `mahype/homebrew-torro` anlegen (leer mit README).
- [ ] Personal Access Token mit Schreibrecht auf den Tap (klassisch: Scope `repo`; feingranular: Contents
      read/write nur für den Tap) als Secret `HOMEBREW_TAP_TOKEN` im Repository TorroWorkplace hinterlegen.
- [ ] In `dist-workspace.toml` `tap = …` und `publish-jobs = ["homebrew"]` einkommentieren, `dist generate`.
- [ ] `packaging/homebrew/torro-suite.rb` (URL und Prüfsumme nach dem ersten Release setzen) in `Formula/` des
      Taps legen.

### AUR

- [ ] AUR-Konto, SSH-Schlüsselpaar nur für CI erzeugen, öffentlichen Teil im AUR-Konto eintragen.
- [ ] Secrets `AUR_SSH_PRIVATE_KEY`, `AUR_USERNAME`, `AUR_EMAIL`.
- [ ] Prüfen, ob die Paketnamen `torroworkplace-bin`, `torrocalendar-bin`, `torrotasks-bin`, `torro-suite` frei sind.
- [ ] Neuesten Tag von `KSXGitHub/github-actions-deploy-aur` prüfen und in `publish-aur.yml` eintragen
      (Entwurf nutzt `v4.1.1`, nicht verifiziert).
- [ ] In `dist-workspace.toml` `publish-jobs` um `"./publish-aur"` erweitern, `dist generate`.
- [ ] Erste Version einmal von Hand pushen oder dem Action-Lauf überlassen (er legt das Repository an).

### macOS

Gleiche Secrets wie bei TorroMail (dort `docs/RELEASING.md`); die Werte sind dieselben, sie müssen nur in
diesem Repository noch einmal eingetragen werden.

| Secret | Inhalt |
|---|---|
| `MACOS_CERTIFICATE_P12` | Base64 des exportierten Zertifikats „Developer ID Application“ (`.p12`): `base64 -i cert.p12 \| gh secret set MACOS_CERTIFICATE_P12` |
| `MACOS_CERTIFICATE_PASSWORD` | Passwort, mit dem die `.p12` exportiert wurde |
| `APPLE_ID` | Apple-ID (E-Mail) des Entwicklerkontos |
| `APPLE_TEAM_ID` | Team-ID (10 Zeichen) |
| `APPLE_APP_SPECIFIC_PASSWORD` | App-spezifisches Passwort von appleid.apple.com (für `notarytool`) |

Achtung: Die `.p12` muss „Developer ID Application“ enthalten, nicht „Apple Development“. Prüfen mit
`openssl pkcs12 -legacy -in cert.p12 -nokeys | openssl x509 -noout -subject -enddate`.

- [x] Apple Developer Program, Zertifikat „Developer ID Application“ (vorhanden, wie TorroMail).
- [x] Die fünf Secrets oben in diesem Repository eingetragen (2026-10-11).
- [x] Repository-Variable `MACOS_APP_ENABLED` auf `true` gesetzt (2026-10-11, Schalter im Workflow).
- [x] `publish-jobs` um `"./publish-macos-app"` erweitert: Nach jedem Release entstehen signierte und
      notarisierte `Torro Workplace.app`, `Torro Calendar.app`, `Torro Tasks.app` (arm64 und x86_64).
- [ ] Echte Icons (1024-px-PNG) in `packaging/macos/icons/` ablegen und an `make-app.sh` übergeben.
- [ ] Vor 2027-02-01: Zertifikat erneuern (siehe oben).

### Windows (später)

Was Windows bis zur vollen Unterstützung fehlt (Named Pipes, Konsolenfenster, Tray, GlazeWM, Signatur):
[tasks/plattformen.md](tasks/plattformen.md).


- [x] Entschieden (2026-10-12): unsigniert. Gründe und Alternativen: [tasks/plattformen.md](tasks/plattformen.md), X.10.
- [ ] Repository `mahype/scoop-torro` anlegen, wenn Windows unterstützt wird.

## Offene Fragen

1. **Name des Metapakets.** `docs/research.md` nennt AUR `torroworkplace` als Metapaket; der Entwurf nutzt
   `torro-suite` (AUR und Homebrew), weil `torroworkplace` schon der Kern ist. Einverstanden?
2. **Bundle-ID.** Der Entwurf nutzt `io.github.mahype.torro-calendar` usw. Sie darf sich nach dem ersten
   Release nicht mehr ändern (Schlüsselbund, Einstellungen). Gibt es eine eigene Domain?
3. **Minimale Linux-Basis.** Ubuntu 22.04 (glibc 2.35) als Build-System ist der Standard von `dist`. Reicht
   das, oder soll es eine ältere Basis sein (Container)?
4. **Intel-Macs.** Lohnt `x86_64-apple-darwin` bis Herbst 2027? Ohne den Eintrag spart es einen Build.
5. **Windows-Konsolenfenster.** Die GUI-Binaries haben kein `#![windows_subsystem = "windows"]`; unter Windows
   würde neben dem Fenster eine Konsole aufgehen. Gehört in eine Rust-Aufgabe, nicht in dieses Gerüst.
6. **Sichtbarkeit des Repositories.** Der ARM-Runner `ubuntu-22.04-arm` war bei Public-Repositories kostenlos
   (seit August 2025 allgemein verfügbar); bei privaten Repositories gelten Minuten-Kontingente.
7. **Echte Icons** für alle drei Apps; die SVGs sind Platzhalter.
8. **Ein Installer für alles.** Der Shell-Installer von `dist` gilt je App. Reicht ein `install.sh` im
   Repository, das die drei Installer nacheinander aufruft, oder soll es ein Sammel-Release geben?

## Quellen

Alle Seiten am 2026-10-11 abgerufen, außer wo anders vermerkt.

| Thema | Quelle |
|---|---|
| `dist` Releases (0.33.0 vom 2026-09-10, 0.32.0 vom 2026-05-21, 0.31.0 vom 2026-02-23) | https://github.com/axodotdev/cargo-dist/releases |
| `dist` Handbuch, Konfiguration (Ziele, Installer, `tap`, `publish-jobs`, `pr-run-mode`) | https://axodotdev.github.io/cargo-dist/book/reference/config.html |
| `dist` Workspaces, Tag-Formate (`v1.0.0` = alle Pakete) | https://axodotdev.github.io/cargo-dist/book/workspaces/workspace-guide.html |
| `dist` Homebrew-Installer (Tap, `HOMEBREW_TAP_TOKEN`, keine Casks) | https://axodotdev.github.io/cargo-dist/book/installers/homebrew.html |
| `dist` MSI-Installer (WiX v3) | https://axodotdev.github.io/cargo-dist/book/installers/msi.html |
| `dist` Windows-Signierung (SSL.com, Azure Artifact Signing, nur x86_64) | https://axodotdev.github.io/cargo-dist/book/supplychain-security/signing/windows.html |
| `dist` macOS-Signierung: Quelltext `cargo-dist` 0.32.0, `src/sign/macos.rs` (crates.io) | https://crates.io/crates/cargo-dist/0.32.0 |
| Apple `notarytool`, Stapling | https://developer.apple.com/documentation/security/customizing-the-notarization-workflow (Seitentext nicht abrufbar; Angaben aus dem Werkzeug bekannt, vor dem ersten Lauf prüfen) |
| Homebrew-Taps (Namensregel, `Formula/`, `Casks/`, Vertrauen) | https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap |
| Scoop-Buckets | https://github.com/ScoopInstaller/Scoop/wiki/Buckets |
| AUR-Deploy-Action (Eingaben) | https://github.com/KSXGitHub/github-actions-deploy-aur |
| `cargo-packager` (Formate) | https://github.com/crabnebula-dev/cargo-packager |
| `cargo-bundle` (Formate) | https://github.com/burtonageo/cargo-bundle |
| GitHub: arm64-Runner für öffentliche Repositories allgemein verfügbar (2025-08-07) | https://github.blog/changelog/2025-08-07-arm64-hosted-runners-for-public-repositories-are-now-generally-available/ |
| GitHub: letzter Intel-Runner `macos-15-intel`, Ende Herbst 2027 (2025-09-19) | https://github.blog/changelog/2025-09-19-github-actions-macos-13-runner-image-is-closing-down/ |
