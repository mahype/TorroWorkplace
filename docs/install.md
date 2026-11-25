# Apps herunterladen und installieren

**Downloads:** [GitHub Releases](https://github.com/mahype/TorroWorkplace/releases/latest). Dort die
Tabelle **App downloads** oder `downloads.md` öffnen und App, Betriebssystem und Architektur wählen.
Solange kein Maintainer-Release veröffentlicht ist, gibt es noch keine geprüften Installer-Downloads.
Neun Apps: Torro Workplace (Steuerzentrale), Notification Hub, Workplace Mail, App Feed, Calendar,
Tasks, Assistant, Chat und Contacts. Jede läuft ohne Steuerzentrale; Dienst und nötige Begleitprogramme kommen mit.

## macOS: Disk-Image

Apple Silicon: `aarch64-apple-darwin`, Intel: `x86_64-apple-darwin`. Passendes `.dmg` öffnen und
**alle** angezeigten Apps auf den Applications-Link ziehen, dann Image auswerfen. Die App aus
Applications öffnen, nicht aus dem Image. Für systemweite Applications kann macOS ein Passwort fragen.

Jedes Bündel enthält den Hintergrunddienst. Mail und App Feed liefern zusätzlich Notification Hub fürs
Verfassen, Calendar zusätzlich Torro Reminder. Images und Apps sind mit Developer ID signiert,
notarisiert und geheftet. Gatekeeper nicht umgehen: Bei einer Warnung Download/Signatur prüfen und
melden. macOS- und Intel-Unterstützung der Apps ist durch das Installationsformat nicht erweitert.

**Update:** Apps schließen, neues Image öffnen und Bündel in Applications ersetzen.
**Entfernen:** Apps und Hintergrunddienst beenden, nicht mehr benötigte Bündel in den Papierkorb
verschieben. Hub behalten, solange Mail oder App Feed installiert ist; Reminder mit Calendar entfernen.
Konten, Einstellungen und persönliche Daten bleiben erhalten.

## Windows: Setup für alle Benutzer

Windows x64: `<bin>-<version>-x86_64-pc-windows-msvc-setup.exe`. Setup starten und Administratorabfrage
bestätigen. Installation ist systemweit in `Program Files\TorroWorkplace`, nicht pro Benutzer.
Startmenü › Torro Workplace enthält für jede ausdrücklich installierte App ihren Eintrag. Workplace
öffnet die Steuerzentrale (`ui`). Dienst und gegebenenfalls Hub liegen daneben und werden nicht erhöht
gestartet; es wird kein Windows-Systemdienst eingerichtet.

Die Setups sind **unsigniert**; SmartScreen kann warnen. Herkunft und SHA-256 prüfen, bevor du dich
bewusst für die Ausführung entscheidest. Kein automatisches Umgehen der Sicherheitswarnung.

**Update:** Neues Setup derselben App ausführen. Es bietet an, belegte Apps und Dienst zu schließen;
bei Ablehnung abbrechen. Neuere gemeinsame Komponenten werden nicht durch ältere ersetzt. Fremde
oder beschädigte Dateien im festen Installationsordner müssen vorab mit ihrem ursprünglichen
Installer entfernt werden. **Entfernen:** Windows › Installierte Apps bzw. `control appwiz.cpl`, dort
die betreffende Torro-App deinstallieren. Gemeinsame Dateien bleiben bis zum letzten Besitzer;
Entfernen einer App entfernt weder Partner-Apps noch persönliche Konten/Daten. App-Unterstützung auf
Windows und eventuelle Plattformlücken bleiben unabhängig vom Setup bestehen.

## Linux: DEB, RPM oder pacman

Voraussetzungen: **glibc >=2.35**, Grafikbibliotheken der Distribution (X11/Wayland, xkbcommon, EGL,
Vulkan-Loader). Der Paketmanager löst die angegebenen Systemabhängigkeiten auf. Für Paketinstallationen
sind Administratorrechte nötig; die Apps und der Hintergrunddienst laufen als normaler Benutzer.

Wähle **ein** Format und **eine** Architektur für alle zusammengehörigen Downloads:

- DEB: Debian/Ubuntu-kompatibel, `amd64` oder `arm64`.
- RPM: Fedora-kompatibel, `x86_64` oder `aarch64`; keine Zusage für jede RPM-Distribution.
- pacman: Arch/Omarchy `x86_64`; `aarch64` ist **Arch Linux ARM, nicht Omarchy**.

Zu jedem App-Paket braucht es **`torroworkplace-bin` mindestens derselben Version**. Mail und App Feed
brauchen außerdem `torroinbox-bin`. Die Downloadtabelle verlinkt diese Begleitpakete. Die Steuerzentrale
ist `torro-workplace` (nur Starter/Icon) plus Dienstpaket. Alle acht anderen Apps heißen `<bin>-bin`.
Beispiel für einen leeren Downloadordner, der ausschließlich die gewählte App samt Begleitpaketen in
einer Version/Architektur enthält:

```sh
sudo apt install ./*.deb
# oder (Fedora):
sudo dnf install ./*.rpm
# oder (Arch/Omarchy/Arch Linux ARM):
sudo pacman -U ./*.pkg.tar.zst
```

**Keine neue Paketquelle:** Updates laden sich nicht automatisch aus einem Torro-Repository. Neue
Pakete einschließlich erforderlicher Begleitpakete herunterladen und denselben Befehl ausführen.
RCs sind Vorabversionen und sortieren vor der zugehörigen stabilen Version. AUR bleibt als Alternative
erhalten (`yay -S torro-suite` oder einzelne Pakete); nicht mit direkten Paketen unterschiedlicher
Herkunft mischen, obwohl die Paketnamen übereinstimmen.

**Start:** App über den Launcher öffnen; sie startet den Dienst selbst. Die mitgelieferte
systemd-**Nutzereinheit** wird nicht automatisch aktiviert. Wer sie bewusst möchte:
`systemctl --user enable --now torroworkplace` (niemals `sudo systemctl` für diesen Dienst).
**Entfernen:** z. B. `sudo apt remove torrocalendar-bin`, `sudo dnf remove torrocalendar-bin` oder
`sudo pacman -R torrocalendar-bin`. Benötigte gemeinsame Pakete behalten; Paketmanager schützen
Abhängigkeiten. `torro-workplace` entfernt nur die Steuerzentrale, nicht den Dienst. Persönliche Daten
werden durch diese Pakete nicht gelöscht. Vor Entfernen des letzten Dienstpakets Apps/Dienst beenden
und eine selbst aktivierte Nutzereinheit mit `systemctl --user disable --now torroworkplace` stoppen.

## Qwen3-TTS selbst bauen (nur NVIDIA)

Die Releases lesen lokal mit Pocket TTS. Qwen3-TTS als zweite lokale Stimme (Issue #764,
[assistent.md](assistent.md), Abschnitt „Vorlesen“) gibt es nur in einem eigenen Build. Es rechnet in einem
eigenen Programm, `torro-qwen-tts` (Issue #808): Der Lese-Helfer des Assistenten startet es, sobald Qwen liest,
und beendet es wieder, wenn eine andere Stimme liest. Das Programm hat einen eigenen Cargo-Workspace mit eigenem
`Cargo.lock` unter `programs/torro-qwen-tts/`; Crane und seine Abhängigkeiten berühren den Workspace der Suite
und die Releases nicht.

Es braucht eine NVIDIA-Grafikkarte mit etwa 8 GB freiem Speicher, zum Bauen von `torro-qwen-tts` das
CUDA-Toolkit (`nvcc`, unter Arch `cuda`) und zum Laufen den NVIDIA-Treiber (`libcuda`) und die Bibliotheken des
Toolkits (`libcublas`, `libcublasLt`, `libcurand`, `libnvrtc`). Nur `torro-qwen-tts` hängt an ihnen: Der
Assistent startet auch ohne sie, zum Zuhören, für den Chat und mit Pocket TTS. Kein Python, kein `cmake`.

Dienst und Assistent bauen in **einem** Aufruf, beide mit der Option, Qwen anzubieten; das Programm in seinem
eigenen Ordner:

```sh
cargo build --release -p torroworkplace -p torroassistant --features torroassistant/qwen-cuda
cargo build --release --manifest-path programs/torro-qwen-tts/Cargo.toml
```

Danach `programs/torro-qwen-tts/target/release/torro-qwen-tts` neben das Programm `torroassistant` legen (dort
sucht der Assistent zuerst, sonst auf `PATH`). `torro-qwen-tts --card` zeigt, ob es die Karte findet.

Ohne die Option im Dienst bietet er Qwen nicht an, auch wenn der Assistent es kann. Fehlt `torro-qwen-tts`, zeigen
die Einstellungen keine Grafikkarte, und der Assistent meldet beim Vorlesen, dass das Programm nicht startet.

Danach in Einstellungen › Assistent › Voice unter „Models“ das Modell „Qwen3-TTS 0.6B“ laden (2,5 GB) und unter
„Read aloud“ „Local: Qwen3-TTS (graphics card)“ wählen. macOS und Windows sind nicht erprobt
([messung-qwen3-tts.md](messung-qwen3-tts.md)).

## Prüfsummen und App-Katalog

Zu jedem Installer liegt `<datei>.sha256`. Linux: `sha256sum -c <datei>.sha256`; macOS:
`shasum -a 256 -c <datei>.sha256`; Windows: `Get-FileHash <datei> -Algorithm SHA256` mit der
veröffentlichten Summe vergleichen. Prüfsummen erkennen beschädigte Downloads, ersetzen keine
Vertrauensentscheidung über die Herkunft.

Die Steuerzentrale zeigt native Pakete als **Managed by DEB / RPM / pacman / Windows Setup**.
Installieren/Aktualisieren verweist auf die Downloads, Entfernen auf den Paketmanager bzw. Windows.
Öffnen bleibt möglich. Der Katalog verändert diese Dateien nicht – auch nicht automatisch ergänzte
Abhängigkeiten oder kaputte Installermarker. Ohne native Marker bleiben bisherige AUR-, Homebrew-,
Scoop- und selbstverwaltete Installationswege erhalten. Bei gemischten Installationen wird der gesamte
Auftrag vor Beginn abgelehnt, wenn eines seiner Ziele extern verwaltet ist.

Build-/Wartungsdetails und offenes Prüfprotokoll: [release.md](release.md).
