#!/usr/bin/env bash
# Builds the suite from the source tree in release mode and installs it for the current user (issue #629), so the
# apps show up in the launcher (Linux) or in Spotlight (macOS). Running it again updates the installation.
#
#   scripts/install-local.sh [--help] [macos-dest]
#
# Linux:   programs to ~/.local/bin, .desktop files and SVG icons from packaging/linux/ to ~/.local/share, the
#          systemd user unit torroworkplace.service (running ~/.local/bin/torroworkplace daemon) to
#          ~/.config/systemd/user, then `systemctl --user enable` and `restart`.
# macOS:   the bundles of scripts/macos-dev-bundles.sh, made from target/release, to ~/Applications (or
#          `macos-dest`).
# Windows: not supported yet, the tray service is missing there (docs/release.md, section Windows).
set -euo pipefail

usage() {
    sed -n '2,/^set -e/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
}

case "${1:-}" in
    -h | --help)
        usage
        exit 0
        ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
# The eight programs of the suite (AppId::program_file in crates/torro-core/src/app.rs); the package of each has the
# same name.
programs="torroworkplace torroinbox torro-inbox-mail torro-inbox-notifications torrocalendar torrotasks torroassistant torrochat"

case "$(uname -s)" in
    Linux | Darwin) ;;
    *)
        echo "install-local.sh: not supported on $(uname -s); the tray service is missing on Windows yet" \
            "(docs/release.md, section Windows)" >&2
        exit 2
        ;;
esac

# Build everything before touching the installation, so a failed build leaves the old version in place.
cargo_args=()
for program in $programs; do
    cargo_args+=(-p "$program")
done
(cd "$root" && cargo build --release "${cargo_args[@]}")

install_macos() {
    local dest="${1:-$HOME/Applications}"
    TORRO_PROGRAMS_DIR="$root/target/release" "$root/scripts/macos-dev-bundles.sh" "$dest"
    echo "install-local.sh: bundles in $dest; start the apps from Spotlight"
}

install_linux() {
    local bin="$HOME/.local/bin"
    local data="${XDG_DATA_HOME:-$HOME/.local/share}"
    local config="${XDG_CONFIG_HOME:-$HOME/.config}"
    local packaging="$root/packaging/linux"

    mkdir -p "$bin" "$data/applications" "$data/icons/hicolor/scalable/apps" "$config/systemd/user"

    # Stop the service first: a running program file cannot be overwritten in place on every file system, and the
    # unit is restarted below anyway.
    if command -v systemctl >/dev/null 2>&1; then
        systemctl --user stop torroworkplace.service 2>/dev/null || true
    fi
    for program in $programs; do
        install -m 755 "$root/target/release/$program" "$bin/$program"
    done
    install -m 644 "$packaging"/*.desktop "$data/applications/"
    install -m 644 "$packaging"/icons/*.svg "$data/icons/hicolor/scalable/apps/"

    # The packaged unit starts /usr/bin/torroworkplace; here the program lives in ~/.local/bin.
    sed 's|^ExecStart=.*|ExecStart=%h/.local/bin/torroworkplace daemon|' "$packaging/torroworkplace.service" \
        > "$config/systemd/user/torroworkplace.service"

    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 \
        && gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true

    if command -v systemctl >/dev/null 2>&1; then
        systemctl --user daemon-reload
        systemctl --user enable torroworkplace.service
        systemctl --user restart torroworkplace.service
    else
        echo "install-local.sh: no systemctl found; start '$bin/torroworkplace daemon' yourself" >&2
    fi

    case ":$PATH:" in
        *":$bin:"*) ;;
        *) echo "install-local.sh: $bin is not in PATH; the launcher entries run the programs by name" >&2 ;;
    esac
    echo "install-local.sh: installed to $bin, $data and $config/systemd/user"
}

case "$(uname -s)" in
    Darwin) install_macos "${1:-}" ;;
    Linux) install_linux ;;
esac
