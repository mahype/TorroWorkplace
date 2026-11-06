# Release-Pipeline

Stand: 2026-11-06 (Pakete je App, #411/#478; Apps eigenständig, #279; Entwurf von 2026-10-11, Aufgabe K.16). Nichts
davon ist veröffentlicht: kein Tag, keine Repositories, keine Secrets. Alles, was ein Secret braucht, ist abgeschaltet,
bis die Checkliste unten abgehakt ist.

**Schritt für Schritt für 0.1.0:** [release-0.1.0.md](release-0.1.0.md).

Ziel: `git tag v0.1.0 && git push --tags` baut die ganze Suite (`torroworkplace`, `torroinbox`, `torro-inbox-mail`,
`torro-inbox-notifications`, `torrocalendar`, `torrotasks`, `torroassistant`) für Linux, macOS und Windows, legt sie in ein GitHub Release und aktualisiert die
Paketquellen.

Jede App ist einzeln installierbar und läuft ohne Hauptanwendung: Jedes Paket bringt den Hintergrunddienst mit
(#411, [einzelne-apps.md](einzelne-apps.md)). Welche Pakete es je System gibt, steht unter „Pakete je App“.

## Empfehlung

`dist` (früher cargo-dist) für das Release, dazu zwei kleine eigene Workflows für das, was `dist` nicht kann.

| Baustein | Aufgabe | Wer |
|---|---|---|
| `dist-workspace.toml` + `.github/workflows/release.yml` | Binärarchive für 5 Ziele, Shell-/PowerShell-Installer, Homebrew-Formeln, GitHub Release | `dist` (Workflow ist generiert, nicht von Hand ändern) |
| `.github/workflows/publish-aur.yml` | AUR-Pakete aktualisieren: Dienst `torroworkplace-bin`, sechs App-Pakete `-bin`, Hauptanwendung `torro-workplace`, `torro-suite` | automatisch nach dem Release („custom publish job“), erst mit der Variable `AUR_ENABLED=true` |
| `.github/workflows/publish-macos-app.yml` | signierte, notarisierte `.app`-Bündel: sieben Apps, jede mit dem Service-Bündel, und die Erinnerung | automatisch nach dem Release („custom publish job“) oder von Hand |
| `packaging/` | `.desktop`-Dateien, Icons, PKGBUILD-Vorlagen, Meta-Formel, Brewfile, Scoop-Manifeste je App | von Hand |

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
| 3 | Maintainer | Commit nach `main` (per PR, `scripts/ci-local.sh` grün), dann `git tag v0.1.0 && git push origin v0.1.0`. Ein Tag mit Suffix (`v0.1.0-rc.1`) wird als Prerelease markiert und aktualisiert Tap und AUR nicht. |
| 4 | CI `plan` | Liest den Tag, entscheidet, welche Apps und Ziele gebaut werden. |
| 5 | CI `build-local-artifacts` | Baut je Ziel `.tar.xz` (Windows `.zip`) mit Binary, Lizenzen, README: macOS arm64 (`macos-14`), macOS x86_64 (`macos-15-intel`), Linux x86_64 (`ubuntu-22.04`), Linux arm64 (`ubuntu-22.04-arm`), Windows (`windows-2022`). |
| 6 | CI `build-global-artifacts` | Erzeugt Shell-/PowerShell-Installer je App, Homebrew-Formeln, `sha256.sum`, `source.tar.gz`. |
| 7 | CI `host` | Legt das GitHub Release an und lädt alle Dateien hoch. |
| 8 | CI Publish-Jobs | macOS-`.app.zip` (signiert, notarisiert) ans Release; AUR, sobald `AUR_ENABLED=true` gesetzt ist (sonst überspringt der Job alles). Noch abgeschaltet: Homebrew-Tap. |
| 9 | CI `announce` | Schließt den Lauf ab. |

Wo die Artefakte landen: GitHub Release `v0.1.0` im Repository (Archive, Installer, Formeln, Prüfsummen);
Tap `mahype/homebrew-torro` (`Formula/*.rb`); AUR-Repositories `torroworkplace-bin`, `torro-workplace`, `torroinbox-bin`,
`torro-inbox-mail-bin`, `torro-inbox-notifications-bin`, `torrocalendar-bin`, `torrotasks-bin`, `torroassistant-bin` und
`torro-suite`.

Pull Requests und Merges starten keinen Workflow (`pr-run-mode = "skip"`, `ci.yml` nur bei Tags und von
Hand): Jede Minute auf GitHub Actions kostet Geld, macOS- und Windows-Runner ein Vielfaches. Die Prüfungen
laufen lokal mit `scripts/ci-local.sh`, automatisch über die Git-Hooks in `.githooks/`. Ein Tag startet das
Release, und das ruft zuerst `ci.yml` auf (`plan-jobs`: clippy, Design-Check und Tests auf Linux, macOS,
Windows). Schlägt eine Prüfung fehl, baut es nichts und veröffentlicht nichts; `release.yml` trägt dafür eine
Handänderung am Job `host` (`allow-dirty = ["ci"]`). Solange diese Zeile gilt, lässt `dist generate` die
`release.yml` unverändert. Zum Neuerzeugen die Zeile kurz auskommentieren, `dist generate`, Zeile zurück,
Handänderung (`host` braucht `custom-ci`) wieder einsetzen, `dist plan --tag v<version>`. Vor einem Tag
lässt sich die CI von Hand starten: `gh workflow run CI --ref main`. Eine geänderte `dist`-Konfiguration prüft `dist plan` lokal.

## Pakete je App

Seit #411 (#478) ist jede App ein eigenes Paket, das den Hintergrunddienst (`torroworkplace`) mitbringt. Die
Hauptanwendung (Tray-Icon, Steuerzentrale, App-Katalog) ist ein Paket für sich; wer nur eine App installiert,
bekommt den Dienst ohne Tray-Icon ([einzelne-apps.md](einzelne-apps.md), Entscheidung 1). Mail und App Feed bringen
zusätzlich den Notification Hub mit, weil sie dessen Fenster zum Verfassen öffnen.

| System | eine App | Hauptanwendung | ganze Suite |
|---|---|---|---|
| macOS (Bündel) | `<App>.app` mit eingebettetem `Torro Workplace Service.app` | `Torro Workplace.app` (ebenfalls mit Service-Bündel) | alle Bündel |
| AUR | `<bin>-bin`, hängt an `torroworkplace-bin` (Mail, App Feed auch an `torroinbox-bin`) | `torro-workplace` (`arch=any`, `.desktop` und Icon, hängt an `torroworkplace-bin`) | `torro-suite` |
| Homebrew | `mahype/torro/<bin>`, `depends_on "mahype/torro/torroworkplace"` (Mail, App Feed auch `torroinbox`) | `mahype/torro/torroworkplace` | `torro-suite` oder Brewfile |
| Scoop | `<bin>`: eigene `.zip` plus die von `torroworkplace` (Mail, App Feed auch `torroinbox`) in einem Ordner | `torroworkplace` | `torro-suite` |

- **AUR:** `torroworkplace-bin` enthält nur das Programm und die systemd-Einheit, keinen Starter-Eintrag. Der
  Eintrag `torro-workplace.desktop` kommt allein mit `torro-workplace`; an ihm erkennt der Dienst die
  Hauptanwendung. Vorlagen: `PKGBUILD.core.in` (Dienst), `PKGBUILD.in` (Apps), `PKGBUILD.main.in`
  (Hauptanwendung), `PKGBUILD.suite.in`.
- **Homebrew:** Die Abhängigkeit steht in `[package.metadata.dist.dependencies.homebrew]` der sechs App-Crates, mit
  `stage = ["run"]`: Ohne Stufe installierte `dist` das Paket auf den Build-Runnern, statt es in die Formel zu
  schreiben. `dist build --artifacts=global` zeigt die Formeln mit `depends_on`. Auf macOS liefert Homebrew nur
  die Programme für das Terminal; Bündel kommen aus dem Release oder dem App-Katalog.
- **Scoop:** Jedes App-Manifest legt `bin` und Startmenü-Eintrag nur für das eigene Programm an, damit sich die
  Kopien des Dienstes nicht in die Quere kommen. Der Eintrag „Torro Workplace“ kommt nur mit `torroworkplace.json`.
- **macOS:** Jede Kopie des Service-Bündels ist gleich (`io.github.mahype.torro-workplace-service`, dasselbe
  Team), sonst fragte der Schlüsselbund je Kopie neu. Die Suite startet die Kopie mit der höchsten
  `TorroVersion` (die ganze Version samt Vorabversion; `CFBundleShortVersionString` nimmt nur Ziffern und Punkte) in `<App>.app/Contents/Library/LoginItems/Torro Workplace Service.app/Contents/Info.plist`.
- Die nackten Archive und Installer je Programm bleiben ohne Dienst: Wer unter Linux oder Windows eine App so von
  Hand lädt, braucht `torroworkplace` dazu, und es entsteht kein Starter-Eintrag.

**Dateinamen im Release** (der App-Katalog sucht genau diese, `<bin>` = `AppId::program_file`):

| System | Datei | Prüfsumme |
|---|---|---|
| Linux | `<bin>-x86_64-unknown-linux-gnu.tar.xz`, `<bin>-aarch64-unknown-linux-gnu.tar.xz` (ein Ordner `<bin>-<ziel>/` mit Programm, Lizenzen, README) | `<datei>.sha256` |
| Windows | `<bin>-x86_64-pc-windows-msvc.zip` (Programm, Lizenzen, README ohne Ordner) | `<datei>.sha256` |
| macOS, Terminal | `<bin>-aarch64-apple-darwin.tar.xz`, `<bin>-x86_64-apple-darwin.tar.xz` | `<datei>.sha256` |
| macOS, Bündel | `<bin>-aarch64-apple-darwin.app.zip`, `<bin>-x86_64-apple-darwin.app.zip`; für die Erinnerung `torro-reminder-<arch>-apple-darwin.app.zip` | `<datei>.sha256` |

Dazu je Programm `<bin>-installer.sh`, `<bin>-installer.ps1`, `<bin>.rb` und für alle `sha256.sum`, `source.tar.gz`.

## Pro Betriebssystem

| System | Ziele | Format | Signierung | Installieren |
|---|---|---|---|---|
| Linux | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | `.tar.xz`; AUR-Pakete | nicht nötig; Prüfsummen im Release | Arch: `yay -S torro-suite`; sonst: Shell-Installer je App oder Tarball |
| macOS | `aarch64-apple-darwin`, `x86_64-apple-darwin` | `.tar.xz` mit Binary; `.app.zip` für jedes Bündel | Binary: optional. `.app`: Developer ID + Notarisierung nötig, damit ein Doppelklick nach Download nicht blockiert | `brew install mahype/torro/torro-suite` |
| Windows | `x86_64-pc-windows-msvc` | `.zip`, PowerShell-Installer | optional; ohne Signatur zeigt SmartScreen eine Warnung | `powershell -c "irm <URL>/torroworkplace-installer.ps1 \| iex"`; später `scoop install` |

Der Shell-Installer legt je App eine Datei in `~/.cargo/bin` (`install-path = "CARGO_HOME"`). Die ganze
Suite heißt also sieben Aufrufe oder das geplante `install.sh` (siehe `docs/research.md`, Abschnitt 7).

### Linux

- Gnu statt musl, bewusst: Die Iced-Apps laden zur Laufzeit per `dlopen` `libwayland-client`,
  `libxkbcommon`, `libX11`, `libvulkan` und `libEGL` (geprüft: `ldd` zeigt nur `libc`, `libm`, `libgcc_s`; die
  übrigen Namen stehen als Zeichenketten im Binary). Ein statisches musl-Binary kann das nicht.
  Der Nutzer braucht die Grafik-Bibliotheken seiner Distribution, sonst nichts.
- Der Build braucht keine Systembibliotheken (`Cargo.lock` enthält weder `libdbus-sys` noch `openssl-sys`;
  Tray und Benachrichtigungen laufen über `zbus`, TLS über `rustls`).
- Die Linux-Binaries entstehen auf Ubuntu 22.04 (glibc 2.35). Ältere Distributionen laufen damit nicht.
- Startmenü: eine `.desktop`-Datei je Eintrag in `packaging/linux/` (`torro-workplace`, `torro-inbox`,
  `torro-inbox-mail`, `torro-inbox-notifications`, `torro-calendar`, `torro-tasks`, `torro-assistant`).
  `Exec` ist die Programmdatei des Eintrags, `StartupWMClass` die App-ID aus `torro-core`
  (`AppId::window_class`), damit Hyprland-Regeln und Launcher das Fenster zuordnen. Die Icons sind die echten
  (`packaging/linux/icons/*.svg`, Variante „red“, flach mit 4 % Rand).
- systemd-Nutzereinheit für den Kern: `packaging/linux/torroworkplace.service` (`/usr/bin/torroworkplace daemon`).
- AUR: sieben `-bin`-Pakete, eines je Programm: `torroworkplace-bin` (nur der Dienst: Programm und
  systemd-Einheit), `torroinbox-bin`, `torro-inbox-mail-bin`, `torro-inbox-notifications-bin`, `torrocalendar-bin`,
  `torrotasks-bin`, `torroassistant-bin` (je `x86_64` und `aarch64`, Quelle ist das Release-Archiv, Prüfsummen setzt
  `updpkgsums`). Jedes App-Paket hängt an `torroworkplace-bin`; Mail und App Feed auch an `torroinbox-bin`, weil sie
  das Verfassen dort öffnen. Die Hauptanwendung ist das Paket `torro-workplace` (`arch=any`, `.desktop`-Eintrag und
  Icon, hängt an `torroworkplace-bin`). Das Metapaket `torro-suite` (`arch=any`, nur `depends`) hängt an allen.
  Vorlagen: `packaging/aur/PKGBUILD.core.in`, `PKGBUILD.in`, `PKGBUILD.main.in`, `PKGBUILD.suite.in`. Der Job nutzt
  `KSXGitHub/github-actions-deploy-aur` (Eingaben `pkgname`, `pkgbuild`, `updpkgsums`, `test`,
  `ssh_private_key`, `commit_username`, `commit_email`). Er hängt am Release (`publish-jobs`), jeder seiner Jobs
  läuft aber nur mit der Repository-Variable `AUR_ENABLED=true`.

### macOS

- `dist` baut nur nackte Binaries und kennt weder `.app`-Bündel noch Notarisierung (im Quelltext von 0.32.0:
  „in the future, this module will also support notarization“). Signieren kann es mit `macos-sign = true`
  und den Secrets `CODESIGN_IDENTITY`, `CODESIGN_CERTIFICATE`, `CODESIGN_CERTIFICATE_PASSWORD`. Das ist für
  die Binaries nicht eingeschaltet.
- Warum die nackten Binaries reichen: Was per `curl | sh` oder `brew` kommt, trägt kein Quarantäne-Attribut,
  Gatekeeper prüft es nicht. Das gilt für den Kern und für Start aus dem Terminal. Ein Doppelklick auf eine
  heruntergeladene GUI-App ohne Notarisierung wird blockiert.
- Für die GUIs gibt es deshalb `publish-macos-app.yml`: lädt das fertige Binary aus dem
  Release, `packaging/macos/make-app.sh` baut das Bündel (Info.plist aus `Info.plist.in`, `.icns` aus dem
  1024-px-PNG in `packaging/macos/icons/`), dann `codesign --options runtime`, `xcrun notarytool submit --wait`,
  `xcrun stapler staple`, `ditto` zu `<bin>-<arch>-apple-darwin.app.zip`, Upload ans Release.
- **Sieben Bündel, jedes mit Service-Bündel, dazu der Reminder** (#279, #411, [lebenszyklus.md](lebenszyklus.md)):
  Torro Workplace, Torro Notification Hub, Torro Workplace Mail, Torro App Feed, Torro Calendar, Torro Tasks und
  Torro Assistant, je mit der Bundle-ID `io.github.mahype.<fensterklasse>`. In jedem steckt unter
  `Contents/Library/LoginItems/` das Service-Bündel „Torro Workplace Service.app“
  (`io.github.mahype.torro-workplace-service`, `LSUIElement`, ohne Dock-Icon, gebaut aus dem Archiv
  `torroworkplace-<arch>-apple-darwin.tar.xz`): Aus ihm läuft der Dienst, damit jede App ohne Hauptanwendung
  läuft und ein Doppelklick auf „Torro Workplace“ bei laufendem Dienst die Steuerzentrale öffnet. Dazu kommt
  „Torro Reminder.app“ (Agent, das Binary des Kalenders als `torro-reminder`, ohne Service-Bündel, das bringt der
  Kalender mit). Die Matrix des Workflows hat je Eintrag zwei Zeilen (arm64 und x86_64); Einträge mit
  `service: "1"` laden das Archiv des Dienstes dazu. Jedes Bündel wird um das Programm des Dienstes größer.
- **Signaturreihenfolge** (von innen nach außen): erst das eingebettete Service-Bündel (sein Programm, dann das
  Bündel), dann das Programm des äußeren Bündels, zuletzt das äußere Bündel; sonst bricht dessen Siegel.
  `codesign --verify --deep --strict` prüft alles zusammen. Notarisiert wird jedes äußere Bündel einzeln, mit dem
  eingebetteten.
- **Icons:** die echten, Variante „red“ aus `docs/prototypes/icons/red/` (AP.6/AP.7). macOS mit Big-Sur-Rand und
  Schatten (`packaging/macos/icons/*.png`, 1024 px), Linux und Windows flach mit 4 % Rand
  (`packaging/linux/icons/*.svg`, `packaging/windows/*.ico`). Das Service-Bündel trägt das Icon des Workplace,
  der Reminder das des Kalenders.
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
  der Dienst beim ersten Lesen um. **Mit #279 ändert sich die Signatur-Kennung des Dienstes im Release** auf die
  Bundle-ID des Service-Bündels (`io.github.mahype.torro-workplace-service`): Der Schlüsselbund fragt einmal neu.
  Das ist hinnehmbar, weil es bisher kein Bündel-Release gab, also keine Kennung, die sich ändert; ab jetzt darf
  sie sich nicht mehr ändern (offene Frage 2).
- **Entwicklungs-Builds** (X.14): Debug-Builds sind ad hoc signiert, mit einer Kennung, die sich mit jedem Build
  ändert; der Schlüsselbund erkennt sie danach nicht wieder. Einmal `scripts/macos-dev-cert.sh` ausführen (legt
  das Zertifikat „Torro Development“ an und vertraut ihm für Codesignatur, fragt nach dem Passwort). Danach
  signiert `cargo run` über den Runner in `.cargo/config.toml` jedes Programm mit diesem Zertifikat und der
  Kennung `io.github.mahype.dev.<name>`; wer die Binaries anders startet, signiert sie mit
  `scripts/macos-dev-sign.sh`. Ohne das Zertifikat (CI, andere Rechner) tun beide nichts.
  Die Entwicklungs-Bündel baut `scripts/macos-dev-bundles.sh` aus `target/debug` (acht Bündel, sieben davon mit dem
  Service-Bündel, auch in einen übergebenen Ordner); es signiert mit diesem Zertifikat und ohne es ad hoc als
  Ganzes, damit `codesign --verify` die Bündel annimmt. Der Dienst im Service-Bündel behält dabei die Kennung des Programms
  `torroworkplace` (`io.github.mahype.dev.torroworkplace`), damit die Schlüsselbund-Einträge lesbar bleiben.
- Homebrew: `dist` erzeugt nur Formeln (kein Cask). Ein Cask für die `.app.zip` kann später von Hand in
  `Casks/` des Taps liegen; Versions- und Prüfsummen-Update müsste ein eigener Schritt übernehmen.

### Windows

- Archiv `.zip` pro App und PowerShell-Installer. Kein MSI: bräuchte WiX v3 und gäbe drei getrennte MSIs.
- Ohne Signatur zeigt SmartScreen beim Browser-Download eine Warnung. `dist` 0.33.0 kann mit Azure Artifact
  Signing signieren (nur x86_64; Secrets `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`, OIDC,
  GitHub-Environment `release`); früher nur SSL.com eSigner. Für 0.1.0 nicht eingeschaltet.
- Der Tray-Dienst ist unter Windows noch nicht implementiert (`tray/` kennt nur Linux). Das Release baut
  trotzdem, die CI ist auf allen drei Systemen grün.
- Scoop später: ein Bucket `mahype/scoop-torro` mit **einem Manifest je App** (#411, `packaging/scoop/`,
  `checkver`/`autoupdate`): Jedes lädt die `.zip` seiner App und die von `torroworkplace` (Mail und App Feed auch
  `torroinbox`) in einen Ordner, wo die App den Dienst findet; `bin` und `shortcuts` gelten nur für das eigene
  Programm. `torroworkplace.json` ist die Hauptanwendung, `torro-suite.json` hängt an allen. Installieren:
  `scoop install torrocalendar`, `scoop install torroworkplace` oder `scoop install torro-suite`. Mail und App
  Feed sind eigene Programme, weil Windows die Taskleiste nach `.exe` gruppiert und das eingebettete Icon zeigt
  ([lebenszyklus.md](lebenszyklus.md), Entscheidung 8).
- Die Programme sind GUI-Programme ohne Konsolenfenster; die Apps starten den Dienst selbst, losgelöst.

### Homebrew-Tap

- Repository `mahype/homebrew-torro` (Name muss mit `homebrew-` beginnen). `dist` schreibt
  `Formula/torroworkplace.rb`, `torroinbox.rb`, `torro-inbox-mail.rb`, `torro-inbox-notifications.rb`,
  `torrocalendar.rb`, `torrotasks.rb`, `torroassistant.rb` bei jedem Release. Jede App-Formel trägt
  `depends_on "mahype/torro/torroworkplace"` (Mail und App Feed auch `torroinbox`), aus
  `[package.metadata.dist.dependencies.homebrew]` im `Cargo.toml` des Crates.
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
- [ ] `release.yml` nicht von Hand ändern (außer der Handänderung am Job `host`), immer `dist generate` nach
      Änderungen an `dist-workspace.toml`, wie oben unter „Ablauf eines Releases“ beschrieben.

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
- [ ] Prüfen, ob die Paketnamen `torroworkplace-bin`, `torro-workplace`, `torroinbox-bin`, `torro-inbox-mail-bin`,
      `torro-inbox-notifications-bin`, `torrocalendar-bin`, `torrotasks-bin`, `torroassistant-bin`, `torro-suite` frei sind.
- [ ] Neuesten Tag von `KSXGitHub/github-actions-deploy-aur` prüfen und in `publish-aur.yml` eintragen
      (Entwurf nutzt `v4.1.1`, nicht verifiziert).
- [x] `publish-jobs` in `dist-workspace.toml` um `"./publish-aur"` erweitert, `dist generate` (#478).
- [ ] Repository-Variable `AUR_ENABLED` auf `true` setzen (`gh variable set AUR_ENABLED --body true`), erst wenn die
      Punkte oben erledigt sind; bis dahin überspringt das Release die AUR-Jobs.
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
      notarisierte Bündel (arm64 und x86_64) für alle sieben Apps und die Erinnerung; jedes App-Bündel trägt
      das Service-Bündel (#279, #411).
- [x] Echte Icons (1024-px-PNG) in `packaging/macos/icons/`, im Workflow und in `scripts/macos-dev-bundles.sh`
      an `make-app.sh` übergeben (AP.7, #279).
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
5. **Windows-Konsolenfenster.** Erledigt (#279): Die GUI-Programme tragen `windows_subsystem = "windows"`, ein
   Startfehler landet in einer Datei im State-Verzeichnis.
6. **Sichtbarkeit des Repositories.** Der ARM-Runner `ubuntu-22.04-arm` war bei Public-Repositories kostenlos
   (seit August 2025 allgemein verfügbar); bei privaten Repositories gelten Minuten-Kontingente.
7. **Echte Icons** für alle Apps. Erledigt (#279, AP.6/AP.7): Variante „red“, für alle sieben Apps; siehe
   „macOS“ und `packaging/README.md`.
8. **Ein Installer für alles.** Der Shell-Installer von `dist` gilt je App. Reicht ein `install.sh` im
   Repository, das die sieben Installer nacheinander aufruft, oder soll es ein Sammel-Release geben?

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
