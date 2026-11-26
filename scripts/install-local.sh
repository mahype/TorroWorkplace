#!/usr/bin/env bash
# Builds the suite from the source tree in release mode and installs it for the current user (issue #629), so the
# apps show up in the launcher (Linux) or in Spotlight (macOS). Running it again updates the installation and
# restarts the core's service with the new program (issue #1001).
#
#   scripts/install-local.sh [--help] [--restart-apps] [macos-dest]
#
# Linux:   first the systemd user unit torroworkplace.service (running ~/.local/bin/torroworkplace daemon, only the
#          service itself is stopped: KillMode=process) to ~/.config/systemd/user, `daemon-reload` when it changed or
#          systemd asks for it (NeedDaemonReload=yes), `enable` when it is not. When a drop-in makes the unit start
#          another program, it stops here, before anything is installed. Then the programs to ~/.local/bin, the
#          .desktop files and SVG icons from packaging/linux/ to ~/.local/share, `restart`, and a check that a new
#          service process runs the new program.
# macOS:   the bundles of scripts/macos-dev-bundles.sh, made from target/release, to ~/Applications (or
#          `macos-dest`). The service starts at login through the LaunchAgent the setting "Start at login"
#          writes: when it is missing or starts another program, `torroworkplace login-item enable` of the new
#          service switches the setting on. When launchd has an agent loaded that runs another program, it is
#          unloaded (`bootout`) first. Then the service is restarted (`launchctl kickstart -k`, or `bootstrap` when
#          the agent is not loaded), up to three times, since macOS may kill the first start of a replaced program
#          (OS_REASON_CODESIGNING), and checked: a new process whose program file is the new one.
# Both:    a `torroworkplace daemon` of this user started outside systemd or launchd (by an app) is ended first.
#          Apps still running a replaced program of the suite are named afterwards. With --restart-apps they are
#          ended (SIGTERM, as if closed: input not saved in them is lost) and started again; compose windows,
#          the reminder, MCP servers and the service's helpers are only named, never ended. MCP configurations are
#          not touched.
# Windows: not supported yet, the tray service is missing there (docs/release.md, section Windows).
#
# Exit codes: 0 done, 1 the service does not run with the new program (or a unit conflict stopped the install),
# 2 unsupported system, 64 bad arguments. Runs with the bash 3.2 of macOS.
set -euo pipefail

usage() {
    sed -n '2,/^set -e/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
os="$(uname -s)"
uid="$(id -u)"
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

# --- Processes -------------------------------------------------------------------------------------------------
# A process is told by what it is, not by its command line: its owner, the program file it runs (Linux
# /proc/<pid>/exe, " (deleted)" once replaced; macOS the first program file lsof names, since `ps -o comm=` only shows
# argv[0]) and its first argument. Its start time tells it apart from a later process with the same pid.

# Whether the process `pid` belongs to this user.
process_owned() {
    case "$os" in
        Linux) [ -O "$proc/$1" ] ;;
        *) [ "$(ps -o uid= -p "$1" 2>/dev/null | tr -d ' ' || true)" = "$uid" ] ;;
    esac
}

# The program file of `pid`, empty when it cannot be read.
process_exe() {
    case "$os" in
        Linux) readlink "$proc/$1/exe" 2>/dev/null || true ;;
        *) { lsof -a -p "$1" -d txt -Fn 2>/dev/null || true; } | awk '/^n/ { print substr($0, 2); exit }' ;;
    esac
}

# When `pid` started, in the system's own form; only compared, never shown.
process_start() {
    case "$os" in
        Linux) { cat "$proc/$1/stat" 2>/dev/null || true; } | awk '{ sub(/.*\) /, ""); print $20 }' ;;
        *) ps -o lstart= -p "$1" 2>/dev/null || true ;;
    esac
}

# The arguments of `pid` after the program, one per line; read to tell its role or to start it again, never shown.
process_args() {
    local args comm
    case "$os" in
        Linux) { tr '\0' '\n' < "$proc/$1/cmdline"; } 2>/dev/null | sed '1d' || true ;;
        *)
            args="$(ps -o args= -p "$1" 2>/dev/null || true)"
            comm="$(ps -o comm= -p "$1" 2>/dev/null || true)"
            # Without argv[0], which may hold blanks (a bundle path); the arguments of the suite hold none.
            local words=()
            read -r -a words <<< "${args#"$comm"}" || true
            for args in ${words[@]+"${words[@]}"}; do echo "$args"; done
            ;;
    esac
}

# The first argument of `pid` (its subcommand), empty without one.
process_sub() {
    process_args "$1" | sed -n 1p
}

# What identifies `pid` until it ends: its start and its program file.
process_id() {
    echo "$(process_start "$1")|$(process_exe "$1")"
}

# Sends SIGTERM to `pid` only when it is still the process `id` (process_id) of this user.
end_process() {
    local pid="$1" id="$2"
    process_owned "$pid" || return 1
    [ "$(process_id "$pid")" = "$id" ] || return 1
    kill "$pid" 2>/dev/null
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

# Whether `pid` is a service of this user: its program file is `torroworkplace` (also a replaced one) and its
# subcommand is `daemon`.
is_service() {
    local exe
    process_owned "$1" || return 1
    exe="$(process_exe "$1")"
    exe="${exe% (deleted)}"
    [ "${exe##*/}" = torroworkplace ] || return 1
    [ "$(process_sub "$1")" = daemon ]
}

# The services of this user except `keep` (the one systemd or launchd runs), as `pid id` lines (process_id).
# pgrep only proposes; is_service decides.
service_processes() {
    local keep="${1:-}" pid
    for pid in $(pgrep -U "$uid" -f torroworkplace 2>/dev/null || true); do
        [ "$pid" = "$keep" ] && continue
        is_service "$pid" || continue
        echo "$pid $(process_id "$pid")"
    done
    return 0
}

# Ends a service that runs outside systemd or launchd (an app started it): it holds the lock, so the new service
# would give up as "already running". SIGTERM ends it like `systemctl --user stop` does; each one is checked again
# right before the signal.
end_stray_daemons() {
    local found pid id ended=""
    found="$(service_processes "${1:-}")"
    [ -n "$found" ] || return 0
    while read -r pid id; do
        end_process "$pid" "$id" && ended="$ended $pid"
    done <<< "$found"
    [ -n "$ended" ] || return 0
    say "ended the service started outside the system's service manager (pid${ended})"
    # shellcheck disable=SC2086 # one pid per word
    wait_gone $ended || warn "the old service (pid${ended}) did not end; the new one may not start"
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

# Sets the unit up before the old service stops: written, loaded, enabled, and starting `bin/torroworkplace`. Fails
# (1) when a drop-in or another unit of the same name makes it start something else.
prepare_unit_linux() {
    local bin="$1" config="$2" packaging="$3" file
    file="$config/systemd/user/$unit"
    if write_unit "$file" "$packaging/$unit" \
        || [ "$(systemctl --user show -p NeedDaemonReload --value "$unit" 2>/dev/null || true)" = yes ]; then
        say "unit $file changed, reloading systemd"
        if ! systemctl --user daemon-reload; then
            warn "systemctl --user daemon-reload failed; nothing was installed"
            return 1
        fi
    fi
    if ! systemctl --user is-enabled --quiet "$unit" 2>/dev/null; then
        say "enabling $unit"
        if ! systemctl --user enable "$unit"; then
            warn "systemctl --user enable $unit failed; nothing was installed"
            return 1
        fi
    fi
    case "$(systemctl --user show -p ExecStart "$unit" 2>/dev/null || true)" in
        *"path=$bin/torroworkplace "*) return 0 ;;
    esac
    warn "$unit does not start $bin/torroworkplace; drop-ins:" \
        "$(systemctl --user show -p DropInPaths --value "$unit" 2>/dev/null || true)" \
        "(see: systemctl --user cat $unit). Nothing was installed."
    return 1
}

# Whether the unit runs `program` in a process other than `old` and still does a pause later; prints the pid.
unit_runs() {
    local program="$1" old="${2:-}" tries=0 pid
    while [ "$tries" -lt 5 ]; do
        tries=$((tries + 1))
        pid="$(unit_pid)"
        if [ -n "$pid" ] && [ "$pid" != "$old" ] && [ "$(process_exe "$pid")" = "$program" ]; then
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

# Restarts the systemd user unit and checks that a new process runs `bin/torroworkplace`; `old` is the service's
# pid before the install.
restart_service_linux() {
    local bin="$1" old="${2:-}" pid out
    end_stray_daemons
    if ! out="$(systemctl --user restart "$unit" 2>&1)"; then
        warn "systemctl --user restart $unit failed: $out (see: journalctl --user -u $unit)"
        status=1
        return 0
    fi
    if pid="$(unit_runs "$bin/torroworkplace" "$old")"; then
        say "service restarted: $bin/torroworkplace (pid $pid)"
    else
        warn "no new service process runs $bin/torroworkplace after the restart (see: journalctl --user -u $unit)"
        status=1
    fi
}

install_linux() {
    local bin="$HOME/.local/bin"
    local data="${XDG_DATA_HOME:-$HOME/.local/share}"
    local config="${XDG_CONFIG_HOME:-$HOME/.config}"
    local packaging="$root/packaging/linux"
    local program old="" systemd=0

    mkdir -p "$bin" "$data/applications" "$data/icons/hicolor/scalable/apps" "$config/systemd/user"

    command -v systemctl >/dev/null 2>&1 && systemd=1
    if [ "$systemd" = 1 ]; then
        # The unit first: a stop with the old unit (KillMode=control-group) would end the apps the service started.
        prepare_unit_linux "$bin" "$config" "$packaging" || exit 1
        old="$(unit_pid)"
        # Stop the service first: a running program file cannot be overwritten in place on every file system, and
        # the unit is restarted below anyway.
        systemctl --user stop "$unit" 2>/dev/null || true
    else
        write_unit "$config/systemd/user/$unit" "$packaging/$unit" || true
    fi
    for program in $programs; do
        install -m 755 "$root/target/release/$program" "$bin/$program"
    done
    install -m 644 "$packaging"/*.desktop "$data/applications/"
    install -m 644 "$packaging"/icons/*.svg "$data/icons/hicolor/scalable/apps/"

    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 \
        && gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true

    if [ "$systemd" = 1 ]; then
        restart_service_linux "$bin" "$old"
    else
        warn "no systemctl found; the service was not restarted: start '$bin/torroworkplace daemon' yourself"
        status=1
    fi

    case ":$PATH:" in
        *":$bin:"*) ;;
        *) warn "$bin is not in PATH; the launcher entries run the programs by name" ;;
    esac
    say "installed to $bin, $data and $config/systemd/user"
}

# Processes of this user still running a replaced program of the suite from `bin`: `pid<TAB>program<TAB>id` lines.
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
        pid="${pid##*/}"
        process_owned "$pid" || continue
        printf '%s\t%s\t%s\n' "$pid" "$program" "$(process_id "$pid")"
    done
    return 0
}

# Ends the app `pid` (still the process `id`) running `program` from `bin` and starts it again with the same
# arguments, detached from this shell.
restart_app_linux() {
    local pid="$1" program="$2" bin="$3" id="$4" arg
    local args=()
    # Read before the process ends; a newline inside an argument would split it, which the suite's never hold.
    while IFS= read -r arg; do
        args+=("$arg")
    done <<< "$(process_args "$pid")"
    [ -n "${args[0]:-}" ] || args=()
    end_process "$pid" "$id" || { warn "$program (pid $pid) ended or changed meanwhile; not restarted"; return 0; }
    wait_gone "$pid" || { warn "$program (pid $pid) did not end; restart it yourself"; return 0; }
    (cd "$HOME" && setsid "$bin/$program" ${args[@]+"${args[@]}"} >/dev/null 2>&1 < /dev/null &)
    say "restarted $program"
}

# --- macOS -----------------------------------------------------------------------------------------------------

# The pid launchd runs for `target` (gui/<uid>/<label>), empty when the agent is not loaded or not running.
launchd_pid() {
    { launchctl print "$1" 2>/dev/null || true; } | awk '$1 == "pid" && $2 == "=" { print $3; exit }'
}

# The program of the agent launchd has loaded for `target`, empty when none is loaded: the `program` line of
# `launchctl print`, else the first of its `arguments`.
launchd_program() {
    { launchctl print "$1" 2>/dev/null || true; } | awk '
        /^[[:space:]]*program = / { sub(/^[[:space:]]*program = /, ""); print; exit }
        /^[[:space:]]*arguments = \{/ { args = 1; next }
        args { sub(/^[[:space:]]*/, ""); print; exit }'
}

# Whether launchd runs `program` for `target` in a process other than `old`, and still does a pause later; prints
# the pid.
launchd_runs() {
    local target="$1" program="$2" old="${3:-}" tries=0 pid
    while [ "$tries" -lt 5 ]; do
        tries=$((tries + 1))
        sleep "$pause"
        pid="$(launchd_pid "$target")"
        [ -n "$pid" ] && [ "$pid" != "$old" ] || continue
        [ "$(process_exe "$pid")" = "$program" ] || continue
        sleep "$pause"
        if [ "$(launchd_pid "$target")" = "$pid" ]; then
            echo "$pid"
            return 0
        fi
    done
    return 1
}

# Unloads the agent `target` and waits until launchd has let it go; on failure says why.
unload_agent() {
    local target="$1" out tries=0
    out="$(launchctl bootout "$target" 2>&1)" || true
    while launchctl print "$target" >/dev/null 2>&1; do
        tries=$((tries + 1))
        if [ "$tries" -ge 5 ]; then
            warn "launchctl bootout $target did not unload the old agent${out:+: $out}; the service was not restarted"
            return 1
        fi
        sleep "$pause"
    done
    return 0
}

# Makes sure the service's LaunchAgent starts `svc`, the service in the new bundles, and (re)starts it.
restart_service_macos() {
    local svc="$1" line state plist label domain target loaded attempt pid old reason out rc
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
    label="$(basename "$plist" .plist)"
    domain="gui/$uid"
    target="$domain/$label"
    # What launchd has loaded may differ from the file (an earlier unload failed, the file was replaced): checked
    # on every run, not only when the file changes.
    loaded="$(launchd_program "$target")"
    if [ "$state" != current ] || { [ -n "$loaded" ] && [ "$loaded" != "$svc" ]; }; then
        if [ -n "$loaded" ]; then
            [ "$loaded" = "$svc" ] || say "launchd runs the service from another program; loading the agent again"
            unload_agent "$target" || { status=1; return 0; }
        fi
        # No service runs while the setting changes, so none writes settings.json meanwhile.
        end_stray_daemons
        if [ "$state" != current ]; then
            case "$state" in
                missing) say "no LaunchAgent for the service yet; switching \"Start at login\" on ($plist)" ;;
                *) say "the LaunchAgent starts another program; pointing it at the new service ($plist)" ;;
            esac
            if ! line="$("$svc" login-item enable)" || [ "${line%% *}" != current ]; then
                warn "cannot point the LaunchAgent at $svc (login-item enable)"
                status=1
                return 0
            fi
        fi
    else
        end_stray_daemons "$(launchd_pid "$target")"
    fi
    attempt=1
    while [ "$attempt" -le 3 ]; do
        old="$(launchd_pid "$target")"
        rc=0
        if launchctl print "$target" >/dev/null 2>&1; then
            out="$(launchctl kickstart -k "$target" 2>&1)" || rc=$?
        else
            out="$(launchctl bootstrap "$domain" "$plist" 2>&1)" || rc=$?
        fi
        if [ "$rc" = 0 ] && pid="$(launchd_runs "$target" "$svc" "$old")"; then
            say "service restarted: $svc (pid $pid)"
            return 0
        fi
        reason="$(launchctl print "$target" 2>/dev/null | grep -i -e 'last exit reason' -e 'last terminating signal' \
            | sed 's/^[[:space:]]*//' | tr -s ' \t\n' ' ' || true)"
        if [ "$rc" != 0 ]; then
            warn "attempt $attempt: launchctl failed ($rc)${out:+: $out}"
        else
            case "$reason" in
                *CODESIGNING*) warn "attempt $attempt: macOS killed the new service while checking its signature" \
                    "(OS_REASON_CODESIGNING)" ;;
                *) warn "attempt $attempt: no new service process runs $svc${reason:+ (${reason% })}" ;;
            esac
        fi
        attempt=$((attempt + 1))
        [ "$attempt" -le 3 ] && sleep "$((pause * 2))"
    done
    warn "the service did not start after 3 attempts; see: launchctl print $target"
    status=1
}

# The processes of this user whose argv[0] lies inside `dest`, as `pid id` lines (process_id), taken before the
# install: what runs from the bundles now runs the old programs afterwards.
processes_in() {
    local dest="$1" pid comm
    { ps -axo pid=,comm= 2>/dev/null || true; } | while read -r pid comm; do
        case "$comm" in
            "$dest/"*) process_owned "$pid" && echo "$pid $(process_id "$pid")" ;;
        esac
    done
    return 0
}

# The program files the install replaced: the one in Contents/MacOS of each bundle macos-dev-bundles.sh made
# (`bundles`, one path per line).
replaced_programs() {
    local bundle file
    while IFS= read -r bundle; do
        [ -d "$bundle/Contents/MacOS" ] || continue
        for file in "$bundle/Contents/MacOS/"*; do
            [ -f "$file" ] && echo "$file"
        done
    done <<< "$1"
    return 0
}

# Of the processes `before` (processes_in), those that still are the same process and ran one of the `replaced`
# program files (replaced_programs): `pid<TAB>program<TAB>id` lines, since a bundle's path holds blanks.
old_processes_macos() {
    local before="$1" replaced="$2" pid id exe
    [ -n "$before" ] || return 0
    while read -r pid id; do
        exe="${id#*|}"
        printf '%s\n' "$replaced" | grep -qxF -- "$exe" || continue
        [ "$(process_id "$pid")" = "$id" ] && printf '%s\t%s\t%s\n' "$pid" "$exe" "$id"
    done <<< "$before"
    return 0
}

# Ends the app `pid` (still the process `id`) whose program is `path` and opens its bundle again.
restart_app_macos() {
    local pid="$1" path="$2" id="$3" bundle
    bundle="${path%%.app/Contents/MacOS/*}.app"
    end_process "$pid" "$id" || { warn "${bundle##*/} (pid $pid) ended or changed meanwhile; not restarted"; return 0; }
    wait_gone "$pid" || { warn "${bundle##*/} (pid $pid) did not end; restart it yourself"; return 0; }
    if open "$bundle"; then
        say "restarted $(basename "$bundle" .app)"
    else
        warn "cannot open $bundle; start it yourself"
    fi
}

install_macos() {
    local dest="$1" bundles
    bundles="$(TORRO_PROGRAMS_DIR="$root/target/release" "$root/scripts/macos-dev-bundles.sh" "$dest")"
    printf '%s\n' "$bundles"
    # The service inside the main application's bundle (scripts/macos-dev-bundles.sh, issue #411).
    local service="Torro Workplace.app/Contents/Library/LoginItems/Torro Workplace Service.app"
    restart_service_macos "$dest/$service/Contents/MacOS/torroworkplace"
    say "bundles in $dest; start the apps from Spotlight"
    # Sets `replaced` of main (bash scopes it dynamically): the program files this install put in place.
    replaced="$(replaced_programs "$bundles")"
}

# --- Both ------------------------------------------------------------------------------------------------------

# The role of the process `pid` running `program` (a name or a path): `app`, which --restart-apps may restart, or
# what else it is, which is only named.
process_role() {
    local pid="$1" program="$2" sub
    sub="$(process_sub "$pid")"
    case "$program" in
        */torro-reminder) echo reminder; return 0 ;;
    esac
    case "${program##*/}:$sub" in
        torroworkplace:daemon) echo service ;;
        torroworkplace:mcp) echo "MCP server" ;;
        torroworkplace: | torroworkplace:ui) echo app ;;
        torroworkplace:*) echo "service helper" ;;
        torroinbox:--compose) echo "compose window" ;;
        *) echo app ;;
    esac
}

# Names the processes still running a replaced program (`old`: `pid<TAB>program<TAB>id`) by program, role and pid,
# never their arguments, and with `restart` 1 restarts the apps among them.
report_old_apps() {
    local old="$1" restart="$2" bin="$3" pid program id role apps="" others=""
    [ -n "$old" ] || return 0
    while IFS="$(printf '\t')" read -r pid program id; do
        role="$(process_role "$pid" "$program")"
        case "$program" in
            *"/Contents/Library/LoginItems/"*) [ "$role" = app ] && role=service ;;
        esac
        if [ "$role" != app ]; then
            others="$others, ${program##*/} ($role, pid $pid)"
        elif [ "$restart" = 1 ]; then
            case "$os" in
                Linux) restart_app_linux "$pid" "$program" "$bin" "$id" ;;
                *) restart_app_macos "$pid" "$program" "$id" ;;
            esac
        else
            apps="$apps, ${program##*/} (pid $pid)"
        fi
    done <<< "$old"
    [ -z "$apps" ] || warn "still running the old program: ${apps#, }; restart them, or run again with --restart-apps"
    [ -z "$others" ] || warn "still running the old program: ${others#, }; close compose windows yourself once" \
        "they are sent or saved, restart the agents of MCP servers"
    return 0
}

main() {
    local restart_apps=0 dest="" arg before="" replaced=""
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

    case "$os" in
        Linux | Darwin) ;;
        *)
            warn "not supported on $os; the tray service is missing on Windows yet (docs/release.md, section Windows)"
            exit 2
            ;;
    esac

    # Build everything before touching the installation, so a failed build leaves the old version in place.
    local cargo_args=() program
    for program in $programs; do
        cargo_args+=(-p "$program")
    done
    (cd "$root" && cargo build --release "${cargo_args[@]}")

    case "$os" in
        Darwin)
            dest="${dest:-$HOME/Applications}"
            before="$(processes_in "$dest")"
            install_macos "$dest"
            report_old_apps "$(old_processes_macos "$before" "$replaced")" "$restart_apps" ""
            ;;
        Linux)
            install_linux
            report_old_apps "$(old_processes_linux "$HOME/.local/bin")" "$restart_apps" "$HOME/.local/bin"
            ;;
    esac
    exit "$status"
}

# Sourced (by a test), it only defines the functions.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    main "$@"
fi
