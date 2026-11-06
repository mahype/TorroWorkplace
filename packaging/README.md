# Packaging

Every app is a package of its own that brings the background service, and one command installs the whole
suite (issue #411). Background, release flow and checklist: [docs/release.md](../docs/release.md). Everything
here is a draft until the maintainer has created the repositories and secrets listed there.

| Directory | Content | Used by |
|---|---|---|
| `linux/` | one `.desktop` file and SVG icon per app, systemd user unit | AUR packages, manual installs, the app catalog |
| `aur/` | `PKGBUILD.core.in` (`torroworkplace-bin`: the service, program and systemd unit), `PKGBUILD.in` (a `-bin` package per app, each depending on `torroworkplace-bin`), `PKGBUILD.main.in` (`torro-workplace`: the main application's `.desktop` entry and icon), `PKGBUILD.suite.in` (`torro-suite` meta package for all of them) | `.github/workflows/publish-aur.yml` |
| `macos/` | `make-app.sh`, `Info.plist.in`, `icons/` (1024 px PNG per app): wrap a GUI binary into a `.app`, optionally with an embedded bundle (the core's service) | `.github/workflows/publish-macos-app.yml`, `scripts/macos-dev-bundles.sh` |
| `windows/` | `.ico` per program, embedded by each crate's `build.rs` | `cargo build` for Windows targets |
| `homebrew/` | meta formula `torro-suite.rb` and `Brewfile` for the tap (the formulae of the seven programs come from `dist`; each app's formula depends on `torroworkplace`) | copy into `mahype/homebrew-torro` once |
| `scoop/` | one Scoop manifest per app (its zip plus `torroworkplace`), `torroworkplace.json` for the main application, `torro-suite.json` for all (Windows, later) | copy into `mahype/scoop-torro` once |

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
highest `CFBundleShortVersionString`. Sign the embedded service before the outer bundle. To check all bundles by hand
without a release, build the workspace and run `scripts/macos-dev-bundles.sh [folder]` (default
`target/debug/bundles`); it makes the eight bundles, seven of them with the service inside, and signs them with the
development identity when there is one (`scripts/macos-dev-cert.sh`), else ad hoc. What to check by hand with them:
"Von Hand prüfen" in [docs/lebenszyklus.md](../docs/lebenszyklus.md).

The per-app Homebrew formulae are not written by hand: `dist` generates them on every release
(see `dist-workspace.toml`); their dependency on `torroworkplace` comes from `[package.metadata.dist.dependencies.homebrew]`
in each app crate's `Cargo.toml`.
