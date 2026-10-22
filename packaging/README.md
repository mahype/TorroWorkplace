# Packaging

One command installs the whole suite. Background, release flow and checklist:
[docs/release.md](../docs/release.md). Everything here is a draft until the maintainer has created
the repositories and secrets listed there.

| Directory | Content | Used by |
|---|---|---|
| `linux/` | one `.desktop` file and SVG icon per app, systemd user unit | AUR packages, manual installs |
| `aur/` | `PKGBUILD.in` templates (a `-bin` package per program, seven in all; `torro-suite` meta package for all of them) | `.github/workflows/publish-aur.yml` |
| `macos/` | `make-app.sh`, `Info.plist.in`, `icons/` (1024 px PNG per app): wrap a GUI binary into a `.app`, optionally with an embedded bundle (the core's service) | `.github/workflows/publish-macos-app.yml`, `scripts/macos-dev-bundles.sh` |
| `windows/` | `.ico` per program, embedded by each crate's `build.rs` | `cargo build` for Windows targets |
| `homebrew/` | meta formula `torro-suite.rb` and `Brewfile` for the tap (the formulae of the seven programs come from `dist`) | copy into `mahype/homebrew-torro` once |
| `scoop/` | `torroworkplace.json`: one Scoop manifest for the whole suite, seven Start menu entries (Windows, later) | copy into `mahype/scoop-torro` once |

## One entry per app

Every app is a program of its own with its own launcher and icon (issue #279, [docs/lebenszyklus.md](../docs/lebenszyklus.md)).
The names come from `AppId` in `crates/torro-core/src/app.rs`; the suite finds the apps by them, so they must match.

| App | Program (`program_file`) | Window class, `.desktop`, icon | macOS bundle (`display_name`) |
|---|---|---|---|
| Control centre | `torroworkplace` | `torro-workplace` | `Torro Workplace.app` |
| Core's service | `torroworkplace daemon` | – | `Torro Workplace.app/Contents/Library/LoginItems/Torro Workplace Service.app` (agent, no Dock icon) |
| Notification Hub | `torroinbox` | `torro-inbox` | `Torro Notification Hub.app` |
| Mail | `torro-inbox-mail` | `torro-inbox-mail` | `Torro Workplace Mail.app` |
| App Feed | `torro-inbox-notifications` | `torro-inbox-notifications` | `Torro App Feed.app` |
| Calendar | `torrocalendar` | `torro-calendar` | `Torro Calendar.app` |
| Tasks | `torrotasks` | `torro-tasks` | `Torro Tasks.app` |
| Assistant | `torroassistant` | `torro-assistant` | `Torro Assistant.app` |
| Reminder | `torrocalendar`, as `torro-reminder` | `torro-reminder` | `Torro Reminder.app` (agent) |

The bundle id is `io.github.mahype.<window class>`, the service's `io.github.mahype.torro-workplace-service`. Sign
the embedded service before the outer bundle. To check all bundles by hand without a release, build the workspace
and run `scripts/macos-dev-bundles.sh [folder]` (default `target/debug/bundles`); it makes the nine bundles and signs
them with the development identity when there is one (`scripts/macos-dev-cert.sh`), else ad hoc. What to check by hand
with them: "Von Hand prüfen" in [docs/lebenszyklus.md](../docs/lebenszyklus.md).

The per-app Homebrew formulae are not written by hand: `dist` generates them on every release
(see `dist-workspace.toml`).
