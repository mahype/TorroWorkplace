# Packaging

Every app is a package of its own that brings the background service, and one command installs the whole
suite (issue #411). Background, release flow and checklist: [docs/release.md](../docs/release.md). Everything
here is a draft until the maintainer has created the repositories and secrets listed there.

| Directory | Content | Used by |
|---|---|---|
| `linux/` | one `.desktop` file and SVG icon per app, systemd user unit | AUR packages, manual installs, the app catalog |
| `aur/` | `PKGBUILD.core.in` (`torroworkplace-bin`: the service, program and systemd unit), `PKGBUILD.in` (a `-bin` package per app, each depending on `torroworkplace-bin`), `PKGBUILD.main.in` (`torro-workplace`: the main application's `.desktop` entry and icon), `PKGBUILD.suite.in` (`torro-suite` meta package for all of them) | `.github/workflows/publish-aur.yml` |
| `macos/` | `make-app.sh`, `Info.plist.in`, `icons/` (1024 px PNG per app): wrap a GUI binary into a `.app`, optionally with an embedded bundle (the core's service) | `.github/workflows/publish-macos-app.yml`, `scripts/macos-dev-bundles.sh` |
| `windows/` | `.ico` per program, embedded by each crate's `build.rs`; all-users Inno `setup.iss` | `cargo build`, `installers.py build-windows` |
| `homebrew/` | meta formula `torro-suite.rb` and `Brewfile` for the tap (the formulae of the seven programs come from `dist`; each app's formula depends on `torroworkplace`) | copy into `mahype/homebrew-torro` once |
| `scoop/` | one Scoop manifest per app (its zip plus `torroworkplace`), `torroworkplace.json` for the main application, `torro-suite.json` for all (Windows, later) | copy into `mahype/scoop-torro` once |

## Install locally

`scripts/install-local.sh` builds the seven programs from the source tree (`cargo build --release`) and installs
them for the current user (issue #629); run it again to update. `--help` lists the targets.

- **Linux:** programs to `~/.local/bin`, the `.desktop` files and SVG icons of `linux/` to `~/.local/share`, and
  the systemd user unit `torroworkplace.service` (with `ExecStart=%h/.local/bin/torroworkplace daemon`) to
  `~/.config/systemd/user`, enabled and restarted. `~/.local/bin` must be in `PATH`, the `.desktop` files start
  the programs by name.
- **macOS:** the bundles of `scripts/macos-dev-bundles.sh`, made from `target/release`, to `~/Applications` (another
  folder as the argument).
- **Windows:** not supported yet, the tray service is missing there ([docs/release.md](../docs/release.md)).

## One entry per app

Every app is a program of its own with its own launcher and icon (issue #279, [docs/lebenszyklus.md](../docs/lebenszyklus.md)).
The names come from `AppId` in `crates/torro-core/src/app.rs`; the suite finds the apps by them, so they must match.

| App | Program (`program_file`) | Window class, `.desktop`, icon | macOS bundle (`display_name`) |
|---|---|---|---|
| Control centre | `torroworkplace` | `torro-workplace` | `Torro Workplace.app` |
| Core's service | `torroworkplace daemon` | – | `<App>.app/Contents/Library/LoginItems/Torro Workplace Service.app` in every app bundle except the reminder (agent, no Dock icon) |
| Notification Hub | `torroinbox` | `torro-inbox` | `Torro Notification Hub.app` |
| Mail | `torro-inbox-mail` | `torro-inbox-mail` | `Torro Workplace Mail.app` |
| App Feed | `torro-inbox-notifications` | `torro-inbox-notifications` | `Torro App Feed.app` |
| Calendar | `torrocalendar` | `torro-calendar` | `Torro Calendar.app` |
| Tasks | `torrotasks` | `torro-tasks` | `Torro Tasks.app` |
| Assistant | `torroassistant` | `torro-assistant` | `Torro Assistant.app` |
| Reminder | `torrocalendar`, as `torro-reminder` | `torro-reminder` | `Torro Reminder.app` (agent) |

The bundle id is `io.github.mahype.<window class>`, the service's `io.github.mahype.torro-workplace-service` in every
copy, signed by the same team, so the keychain trusts each copy. The suite starts the copy whose `Info.plist` has the
highest `TorroVersion` (whole version, pre-release included). Sign the embedded service before the outer bundle. To check all bundles by hand
without a release, build the workspace and run `scripts/macos-dev-bundles.sh [folder]` (default
`target/debug/bundles`); it makes the eight bundles, seven of them with the service inside, and signs them with the
development identity when there is one (`scripts/macos-dev-cert.sh`), else ad hoc. What to check by hand with them:
"Von Hand prüfen" in [docs/lebenszyklus.md](../docs/lebenszyklus.md).

The per-app Homebrew formulae are not written by hand: `dist` generates them on every release
(see `dist-workspace.toml`); their dependency on `torroworkplace` comes from `[package.metadata.dist.dependencies.homebrew]`
in each app crate's `Cargo.toml`.

## Native downloads (#589)

`apps.json` is the seven-app installer matrix (stable identity, actual program, launch arguments and
companions). `installers.py` uses only Python's standard library and never downloads or publishes:

```sh
python3 installers.py build-linux --tag v0.1.0 --input ../target/distrib --output ../target/installers
python3 installers.py build-windows --tag v0.1.0 --input ../target/distrib --output ../target/installers
python3 installers.py verify --tag v0.1.0 --input ../target/installers --output ../target/installers
python3 installers.py downloads --tag v0.1.0 --input ../target/installers --output ../target/installers
```

Run from this directory. Builders require dist archives, their `.sha256`, and `plan-dist-manifest.json`
for exactly the requested tag and seven apps. Supply nFPM **2.48.0** (`NFPM`) or Inno Setup **7.1.1**
(`ISCC`) after checking the pinned hashes in `build-installers.yml`. `verify`/`downloads` need the
merged Linux/Windows manifests, all native packages/Setups, and 14 signed/stapled DMGs with checksums.
Outputs are staged until the requested matrix succeeds; unsafe archive paths/links are rejected.

Linux: eight packages per format and architecture, not eight copies of the service. Each program's
adjacent `<program>.torro-package.json` declares schema 1, manager (`deb`, `rpm`, `pacman`), package and
canonical `/usr/bin/<program>` path. The core owns its marker; the Workplace launcher-only package
does not claim that file. Windows Setup owns the corresponding `.exe.torro-package.json` and SemVer
`.version` alongside the shared program in Program Files. The catalog leaves valid AND broken markers
alone; it checks all targets/companions before starting a job. Legacy unmarked installs remain supported.

macOS: `make-dmg.sh` accepts one app plus its companions, validates their signatures/staples, includes
an Applications link and English instructions, signs/notarises/staples/verifies the image. No unsigned
fallback. Bundle archives remain available for existing consumers.

Offline tests: `python3 -m unittest discover -s packaging/tests` from the repository root. Destructive
platform smoke scripts live in `tests/` and require disposable systems plus `TORRO_INSTALLER_SMOKE=1`.
See [docs/release.md](../docs/release.md) for open platform evidence and release gates, and
[docs/install.md](../docs/install.md) for user instructions. No release proof is claimed by unit tests.
