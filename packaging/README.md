# Packaging

One command installs the whole suite. Background, release flow and checklist:
[docs/release.md](../docs/release.md). Everything here is a draft until the maintainer has created
the repositories and secrets listed there.

| Directory | Content | Used by |
|---|---|---|
| `linux/` | `.desktop` files, placeholder icons, systemd user unit | AUR packages, manual installs |
| `aur/` | `PKGBUILD.in` templates (`-bin` per app, `torro-suite` meta package) | `.github/workflows/publish-aur.yml` |
| `macos/` | `make-app.sh`, `Info.plist.in`: wrap a GUI binary into a `.app` | `.github/workflows/publish-macos-app.yml` |
| `homebrew/` | meta formula `torro-suite.rb` and `Brewfile` for the tap | copy into `mahype/homebrew-torro` once |
| `scoop/` | Scoop manifest templates (Windows, later) | copy into `mahype/scoop-torro` once |

The per-app Homebrew formulae are not written by hand: `dist` generates them on every release
(see `dist-workspace.toml`).
