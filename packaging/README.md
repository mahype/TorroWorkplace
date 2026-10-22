# Packaging

One command installs the whole suite. Background, release flow and checklist:
[docs/release.md](../docs/release.md). Everything here is a draft until the maintainer has created
the repositories and secrets listed there.

| Directory | Content | Used by |
|---|---|---|
| `linux/` | `.desktop` files, placeholder icons, systemd user unit | AUR packages, manual installs |
| `aur/` | `PKGBUILD.in` templates (`-bin` per app, `torro-suite` meta package) | `.github/workflows/publish-aur.yml` |
| `macos/` | `make-app.sh`, `Info.plist.in`: wrap a GUI binary into a `.app`, optionally with an embedded bundle | `.github/workflows/publish-macos-app.yml`, `scripts/macos-dev-bundles.sh` |
| `windows/` | placeholder `.ico` per program, embedded by each crate's `build.rs` | `cargo build` for Windows targets |
| `homebrew/` | meta formula `torro-suite.rb` and `Brewfile` for the tap | copy into `mahype/homebrew-torro` once |
| `scoop/` | `torroworkplace.json`: one Scoop manifest for the whole suite (Windows, later) | copy into `mahype/scoop-torro` once |

## One entry per app

Every app is a program of its own with its own launcher and icon (issue #279, [docs/lebenszyklus.md](../docs/lebenszyklus.md)).
The names come from `AppId` in `crates/torro-core/src/app.rs`; the suite finds the apps by them, so they must match.

| App | Program (`program_file`) | Window class, `.desktop`, icon | macOS bundle (`display_name`) |
|---|---|---|---|
| Control centre | `torroworkplace` | `torro-workplace` | `Torro Workplace.app` |
| Core's service | `torroworkplace daemon` | – | `Torro Workplace.app/Contents/Library/Torro Workplace Service.app` (agent, no Dock icon) |
| Notification Hub | `torroinbox` | `torro-inbox` | `Torro Notification Hub.app` |
| Mail | `torro-inbox-mail` | `torro-inbox-mail` | `Torro Workplace Mail.app` |
| App Feed | `torro-inbox-notifications` | `torro-inbox-notifications` | `Torro App Feed.app` |
| Calendar | `torrocalendar` | `torro-calendar` | `Torro Calendar.app` |
| Tasks | `torrotasks` | `torro-tasks` | `Torro Tasks.app` |
| Assistant | `torroassistant` | `torro-assistant` | `Torro Assistant.app` |
| Reminder | `torrocalendar`, as `torro-reminder` | `torro-reminder` | `Torro Reminder.app` (agent) |

The bundle id is `io.github.mahype.<window class>`, the service's `io.github.mahype.torro-workplace-service`. Sign
the embedded service before the outer bundle. To check all bundles by hand without a release, build the workspace
and run `scripts/macos-dev-bundles.sh [folder]` (default `target/debug/bundles`); it signs them with the development
identity when there is one (`scripts/macos-dev-cert.sh`).

The per-app Homebrew formulae are not written by hand: `dist` generates them on every release
(see `dist-workspace.toml`).
