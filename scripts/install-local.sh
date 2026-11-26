#!/usr/bin/env bash
# Builds the suite from the source tree in release mode and installs it for the current user (issue #629), so the
# apps show up in the launcher (Linux) or in Spotlight (macOS). Running it again updates the installation and
# restarts the core's service with the new program (issue #1001).
#
#   scripts/install-local.sh [--help] [--restart-apps] [macos-dest]
#
# Linux:   programs to ~/.local/bin, .desktop files and SVG icons from packaging/linux/ to ~/.local/share, the
#          systemd user unit torroworkplace.service (running ~/.local/bin/torroworkplace daemon) to
#          ~/.config/systemd/user; `daemon-reload` only when the unit changed, `enable` when it is not, then
#          `restart`, and a check that the service runs the new program.
# macOS:   the bundles of scripts/macos-dev-bundles.sh, made from target/release, to ~/Applications (or
#          `macos-dest`). The service starts at login through the LaunchAgent the setting "Start at login"
#          writes: when it is missing or starts another program, `torroworkplace login-item enable` of the new
#          service switches the setting on. Then the service is restarted (`launchctl kickstart -k`, or
#          `bootstrap` when the agent is not loaded), up to three times, since macOS may kill the first start
#          of a replaced program (OS_REASON_CODESIGNING), and checked to run the new program.
# Both:    a service started outside systemd or launchd (by an app) is ended first. Apps still running an old
#          program are named afterwards; with --restart-apps they are restarted. MCP servers are only named:
#          the agent that started them restarts them. MCP configurations are not touched.
# Windows: not supported yet, the tray service is missing there (docs/release.md, section Windows).
#
# Exit codes: 0 done, 1 installed but the service did not start with the new program, 2 unsupported system,
# 64 bad arguments. Runs with the bash 3.2 of macOS.
set -euo pipefail

usage() {
    sed -n '2,/^set -e/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
}

root="$(cd "$(dirname "$0")/.." && pwd)"
# The nine programs of the suite (AppId::program_file in crates/torro-core/src/app.rs); the package of each has the
# same name.
programs="torroworkplace torroinbox torro-inbox-mail torro-inbox-notifications torrocalendar torrotasks torroassistant torrochat torrocontacts"
# The systemd unit (Linux) of the service.
unit="torroworkplace.service"
# Where running processes are looked up on Linux, and how long a pause between checks is; tests change both.
proc="${TORRO_INSTALL_PROC:-/proc}"
pause="${TORRO_INSTALL_PAUSE:-1}"
# 1 when something failed after the programs were installed; the script goes on and ends with exit code 1.
status=0

say() { echo "install-local.sh: $*"; }
warn() { echo "install-local.sh: $*" >&2; }

# Whether `name` is one of the suite's programs.
is_program() {
    case " $programs " in
        *" $1 "*) return 0 ;;
        *) return 1 ;;
    esac
}

# Waits up to `TORRO_INSTALL_WAIT` pauses (default 15) until none of the processes `pids...` runs any more.
wait_gone() {
    local tries=0 alive pid
    while :; do
        alive=""
        for pid in "$@"; do
            kill -0 "$pid" 2>/dev/null && alive="$alive $pid"
        done
        [ -z "$alive" ] && return 0
        tries=$((tries + 1))
        [ "$tries" -ge "${TORRO_INSTALL_WAIT:-15}" ] && return 1
        sleep "$pause"
    done
}

# The processes running `torroworkplace daemon`, one pid per line, except `keep` (the one systemd or launchd runs).
daemon_pids() {
    local keep="${1:-}" pid
    for pid in $(pgrep -f 'torroworkplace daemon' 2>/dev/null || true); do
        [ "$pid" = "$keep" ] || echo "$pid"
    done
}

# Ends a service that runs outside systemd or launchd (an app started it): it holds the lock, so the new service
# would give up as "already running". SIGTERM ends it like `systemctl --user stop` does.
end_stray_daemons() {
    local pids
    pids="$(daemon_pids "${1:-}")"
    [ -n "$pids" ] || return 0
    # shellcheck disable=SC2086 # one pid per word
    say "ending the service started outside the system's service manager (pid" $pids")"
    # shellcheck disable=SC2086
    kill $pids 2>/dev/null || true
    # shellcheck disable=SC2086
    wait_gone $pids || warn "the old service (pid $(echo $pids)) did not end; the new one may not start"
}

# --- Linux -----------------------------------------------------------------------------------------------------

# Writes the unit when it differs from the packaged one with the program in ~/.local/bin; 0 when it wrote it.
write_unit() {
    local file="$1" packaged="$2" wanted
    # The packaged unit starts /usr/bin/torroworkplace; here the program lives in ~/.local/bin.
    wanted="$(sed 's|^ExecStart=.*|ExecStart=%h/.local/bin/torroworkplace daemon|' "$packaged")"
    if [ -f "$file" ] && [ "$(cat "$file")" = "$wanted" ]; then
        return 1
    fi
    printf '%s\n' "$wanted" > "$file"
    return 0
}

# The main process of the unit, empty when it does not run.
unit_pid() {
    local pid
    pid="$(systemctl --user show -p MainPID --value "$unit" 2>/dev/null || true)"
    [ -n "$pid" ] && [ "$pid" != 0 ] && echo "$pid"
    return 0
}

# Whether the unit runs `program` (not a deleted older file) and still does a pause later; prints the pid.
unit_runs() {
    local program="$1" tries=0 pid exe
    while [ "$tries" -lt 5 ]; do
        tries=$((tries + 1))
        pid="$(unit_pid)"
        exe=""
        [ -n "$pid" ] && exe="$(readlink "$proc/$pid/exe" 2>/dev/null || true)"
        if [ "$exe" = "$program" ]; then
            # A service that finds another one running gives up at once: look again.
            sleep "$pause"
            if [ "$(unit_pid)" = "$pid" ]; then
                echo "$pid"
                return 0
            fi
        fi
        sleep "$pause"
    done
    return 1
}

# Sets up and restarts the systemd user unit, then checks it runs `bin/torroworkplace`.
restart_service_linux() {
    local bin="$1" config="$2" packaging="$3" file pid
    file="$config/systemd/user/$unit"
    if ! command -v systemctl >/dev/null 2>&1; then
        write_unit "$file" "$packaging/$unit" || true
        warn "no systemctl found; start '$bin/torroworkplace daemon' yourself"
        return 0
    fi
    if write_unit "$file" "$packaging/$unit" \
        || [ "$(systemctl --user show -p NeedDaemonReload --value "$unit" 2>/dev/null || true)" = yes ]; then
        say "unit $file changed, reloading systemd"
        systemctl --user daemon-reload
    fi
    if ! systemctl --user is-enabled --quiet "$unit" 2>/dev/null; then
        say "enabling $unit"
        systemctl --user enable "$unit"
    fi
    # A drop-in or another unit of the same name may start something else than the file written above.
    case "$(systemctl --user show -p ExecStart "$unit" 2>/dev/null || true)" in
        *"path=$bin/torroworkplace "*) ;;
        *) warn "$unit does not start $bin/torroworkplace (see: systemctl --user cat $unit)" ;;
    esac
    end_stray_daemons
    systemctl --user restart "$unit" || true
    if pid="$(unit_runs "$bin/torroworkplace")"; then
        say "service restarted: $bin/torroworkplace (pid $pid)"
    else
        warn "the service does not run $bin/torroworkplace after the restart (see: journalctl --user -u $unit)"
        status=1
    fi
}

install_linux() {
    local bin="$HOME/.local/bin"
    local data="${XDG_DATA_HOME:-$HOME/.local/share}"
    local config="${XDG_CONFIG_HOME:-$HOME/.config}"
    local packaging="$root/packaging/linux"
    local program

    mkdir -p "$bin" "$data/applications" "$data/icons/hicolor/scalable/apps" "$config/systemd/user"

    # Stop the service first: a running program file cannot be overwritten in place on every file system, and the
    # unit is restarted below anyway.
    if command -v systemctl >/dev/null 2>&1; then
        systemctl --user stop "$unit" 2>/dev/null || true
    fi
    for program in $programs; do
        install -m 755 "$root/target/release/$program" "$bin/$program"
    done
    install -m 644 "$packaging"/*.desktop "$data/applications/"
    install -m 644 "$packaging"/icons/*.svg "$data/icons/hicolor/scalable/apps/"

    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 \
        && gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true

    restart_service_linux "$bin" "$config" "$packaging"

    case ":$PATH:" in
        *":$bin:"*) ;;
        *) warn "$bin is not in PATH; the launcher entries run the programs by name" ;;
    esac
    say "installed to $bin, $data and $config/systemd/user"
}

# Processes still running a program of the suite from `bin` that was replaced: `pid program`, one per line.
old_processes_linux() {
    local bin="$1" link pid target program
    for link in "$proc"/[0-9]*/exe; do
        target="$(readlink "$link" 2>/dev/null)" || continue
        case "$target" in
            "$bin/"*" (deleted)") ;;
            *) continue ;;
        esac
        program="${target#"$bin/"}"
        program="${program% (deleted)}"
        is_program "$program" || continue
        pid="${link%/exe}"
        echo "${pid##*/} $program"
    done
}

# Restarts the app `pid` running `program` from `bin` with the same arguments, detached from this shell.
restart_app_linux() {
    local pid="$1" program="$2" bin="$3" arg name=1
    local args=()
    # The arguments without the program's own name, read before the process ends.
    # A process that ended meanwhile has no cmdline any more: then there is nothing to restart.
    [ -r "$proc/$pid/cmdline" ] || return 0
    while IFS= read -r -d '' arg; do
        [ "$name" = 1 ] && { name=0; continue; }
        args+=("$arg")
    done < "$proc/$pid/cmdline" || true
    kill "$pid" 2>/dev/null || true
    wait_gone "$pid" || { warn "$program (pid $pid) did not end; restart it yourself"; return 0; }
    (cd "$HOME" && setsid "$bin/$program" ${args[@]+"${args[@]}"} >/dev/null 2>&1 < /dev/null &)
    say "restarted $program"
}

# --- macOS -----------------------------------------------------------------------------------------------------

# The pid launchd runs for `target` (gui/<uid>/<label>), empty when the agent is not loaded or not running.
launchd_pid() {
    { launchctl print "$1" 2>/dev/null || true; } | awk '$1 == "pid" && $2 == "=" { print $3; exit }'
}

# Whether launchd runs `program` for `target` and still does a pause later; prints the pid.
launchd_runs() {
    local target="$1" program="$2" tries=0 pid
    while [ "$tries" -lt 5 ]; do
        tries=$((tries + 1))
        sleep "$pause"
        pid="$(launchd_pid "$target")"
        [ -n "$pid" ] || continue
        [ "$(ps -o comm= -p "$pid" 2>/dev/null || true)" = "$program" ] || continue
        sleep "$pause"
        if [ "$(launchd_pid "$target")" = "$pid" ]; then
            echo "$pid"
            return 0
        fi
    done
    return 1
}

# Makes sure the service's LaunchAgent starts `svc`, the service in the new bundles, and (re)starts it.
restart_service_macos() {
    local svc="$1" line state plist label domain target changed=0 attempt pid reason out job
    if [ ! -x "$svc" ]; then
        warn "no service program at $svc; the service was not restarted"
        status=1
        return 0
    fi
    # The new program knows `login-item`; the old one may not, so it is asked only now.
    if ! line="$("$svc" login-item status)"; then
        warn "cannot read the login item ($svc login-item status)"
        status=1
        return 0
    fi
    state="${line%% *}"
    plist="${line#* }"
    if [ "$state" != current ]; then
        case "$state" in
            missing) say "no LaunchAgent for the service yet; switching \"Start at login\" on ($plist)" ;;
            *) say "the LaunchAgent starts another program; pointing it at the new service ($plist)" ;;
        esac
        line="$("$svc" login-item enable)" || { warn "cannot switch \"Start at login\" on"; status=1; return 0; }
        [ "${line%% *}" = current ] || { warn "the LaunchAgent still does not start $svc"; status=1; return 0; }
        changed=1
    fi
    label="$(basename "$plist" .plist)"
    domain="gui/$(id -u)"
    target="$domain/$label"
    # The service launchd runs now: kickstart -k or bootout ends it, it is no stray.
    job="$(launchd_pid "$target")"
    if [ "$changed" = 1 ] && launchctl print "$target" >/dev/null 2>&1; then
        # launchd keeps the program of the file it loaded until the agent is loaded again.
        launchctl bootout "$target" 2>/dev/null || true
        attempt=0
        while launchctl print "$target" >/dev/null 2>&1 && [ "$attempt" -lt 5 ]; do
            attempt=$((attempt + 1))
            sleep "$pause"
        done
        [ -z "$job" ] || wait_gone "$job" || true
    fi
    end_stray_daemons "$job"
    attempt=1
    while [ "$attempt" -le 3 ]; do
        if launchctl print "$target" >/dev/null 2>&1; then
            out="$(launchctl kickstart -k "$target" 2>&1)" || true
        else
            out="$(launchctl bootstrap "$domain" "$plist" 2>&1)" || true
        fi
        if pid="$(launchd_runs "$target" "$svc")"; then
            say "service restarted: $svc (pid $pid)"
            return 0
        fi
        reason="$(launchctl print "$target" 2>/dev/null | grep -i -e 'last exit reason' -e 'last terminating signal' \
            | sed 's/^[[:space:]]*//' | tr -s ' \t\n' ' ' || true)"
        case "$reason" in
            *CODESIGNING*) warn "attempt $attempt: macOS killed the new service while checking its signature" \
                "(OS_REASON_CODESIGNING)" ;;
            *) warn "attempt $attempt: the service did not start with the new program${reason:+ (${reason% })}" \
                "${out:+- launchctl: $out}" ;;
        esac
        attempt=$((attempt + 1))
        [ "$attempt" -le 3 ] && sleep "$((pause * 2))"
    done
    warn "the service did not start after 3 attempts; see: launchctl print $target"
    status=1
}

# The processes of programs inside `dest` as `pid path`, one per line, to tell later which ran before the install.
processes_in() {
    local dest="$1" pid comm
    { ps -axo pid=,comm= 2>/dev/null || true; } | while read -r pid comm; do
        case "$comm" in
            "$dest/"*) echo "$pid $comm" ;;
        esac
    done
}

# Of the processes `before` (from processes_in), those that still run the same path: started before the install,
# they run the replaced program.
old_processes_macos() {
    local before="$1" pid comm
    [ -n "$before" ] || return 0
    while read -r pid comm; do
        [ "$(ps -o comm= -p "$pid" 2>/dev/null || true)" = "$comm" ] && echo "$pid $comm"
    done <<< "$before"
    return 0
}

# Restarts the app `pid` whose program is `path`, through its bundle.
restart_app_macos() {
    local pid="$1" path="$2" bundle
    # The calendar opens the reminder when one is due; opened on its own it would show nothing.
    case "$path" in
        */torro-reminder)
            say "the reminder (pid $pid) runs the old program until it is dismissed"
            return 0
            ;;
    esac
    bundle="${path%%.app/Contents/MacOS/*}.app"
    kill "$pid" 2>/dev/null || true
    wait_gone "$pid" || { warn "$bundle (pid $pid) did not end; restart it yourself"; return 0; }
    if open "$bundle"; then
        say "restarted $(basename "$bundle" .app)"
    else
        warn "cannot open $bundle; start it yourself"
    fi
}

install_macos() {
    local dest="$1"
    TORRO_PROGRAMS_DIR="$root/target/release" "$root/scripts/macos-dev-bundles.sh" "$dest"
    # The service inside the main application's bundle (scripts/macos-dev-bundles.sh, issue #411).
    local service="Torro Workplace.app/Contents/Library/LoginItems/Torro Workplace Service.app"
    restart_service_macos "$dest/$service/Contents/MacOS/torroworkplace"
    say "bundles in $dest; start the apps from Spotlight"
}

# --- Both ------------------------------------------------------------------------------------------------------

# Whether the process running `program` with `args` is an app, not the service's side: `daemon`, an MCP server
# or a helper of the core, and every copy of the service inside a bundle (macOS).
is_app() {
    local program="$1" args="$2" first
    case "$program" in
        *"/Contents/Library/LoginItems/"*) return 1 ;;
    esac
    case "$program" in
        torroworkplace | */torroworkplace)
            read -r first _ <<< "$args" || true
            [ -z "${first:-}" ] || [ "$first" = ui ]
            return
            ;;
    esac
    return 0
}

# Names the apps still running a replaced program and restarts them with `restart`; `old` is `pid program` lines.
report_old_apps() {
    local old="$1" restart="$2" platform="$3" bin="$4" pid program args apps="" others=""
    [ -n "$old" ] || return 0
    while read -r pid program; do
        case "$platform" in
            Linux) args="$({ tr '\0' ' ' < "$proc/$pid/cmdline"; } 2>/dev/null | cut -d' ' -f2- || true)" ;;
            *) args="$(ps -o args= -p "$pid" 2>/dev/null || true)"; args="${args#"$program"}" ;;
        esac
        # Trims the blanks around the arguments.
        read -r args <<< "$args" || true
        if ! is_app "$program" "$args"; then
            others="$others, $(basename "$program") ${args:+$args }(pid $pid)"
        elif [ "$restart" = 1 ]; then
            case "$platform" in
                Linux) restart_app_linux "$pid" "$program" "$bin" ;;
                *) restart_app_macos "$pid" "$program" ;;
            esac
        else
            apps="$apps, $(basename "$program") (pid $pid)"
        fi
    done <<< "$old"
    [ -z "$apps" ] || warn "still running the old program: ${apps#, }; restart them, or run again with --restart-apps"
    [ -z "$others" ] || warn "still running the old program: ${others#, }; restart what started them (the agent," \
        "for an MCP server)"
    return 0
}

main() {
    local restart_apps=0 dest="" arg before=""
    for arg in "$@"; do
        case "$arg" in
            -h | --help)
                usage
                exit 0
                ;;
            --restart-apps) restart_apps=1 ;;
            -*)
                warn "unknown option $arg (see --help)"
                exit 64
                ;;
            *) dest="$arg" ;;
        esac
    done

    case "$(uname -s)" in
        Linux | Darwin) ;;
        *)
            warn "not supported on $(uname -s); the tray service is missing on Windows yet" \
                "(docs/release.md, section Windows)"
            exit 2
            ;;
    esac

    # Build everything before touching the installation, so a failed build leaves the old version in place.
    local cargo_args=() program
    for program in $programs; do
        cargo_args+=(-p "$program")
    done
    (cd "$root" && cargo build --release "${cargo_args[@]}")

    case "$(uname -s)" in
        Darwin)
            dest="${dest:-$HOME/Applications}"
            # Taken before the install: what runs from the bundles now runs the old programs afterwards.
            before="$(processes_in "$dest")"
            install_macos "$dest"
            report_old_apps "$(old_processes_macos "$before")" "$restart_apps" Darwin ""
            ;;
        Linux)
            install_linux
            report_old_apps "$(old_processes_linux "$HOME/.local/bin")" "$restart_apps" Linux "$HOME/.local/bin"
            ;;
    esac
    exit "$status"
}

# Sourced (by a test), it only defines the functions.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    main "$@"
fi
