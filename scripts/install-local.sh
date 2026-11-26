#!/usr/bin/env bash
# Builds the suite from the source tree in release mode and installs it for the current user (issue #629), so the
# apps show up in the launcher (Linux) or in Spotlight (macOS). Running it again updates the installation and
# restarts the core's service with the new program (issue #1001).
#
#   scripts/install-local.sh [--help] [--restart-apps] [macos-dest]
#
# Linux:   first the systemd user unit torroworkplace.service (running ~/.local/bin/torroworkplace daemon; stop and
#          restart end only the service, not the apps it started: KillMode=process) to ~/.config/systemd/user,
#          `daemon-reload` when it changed or systemd asks for it (NeedDaemonReload=yes). When the loaded unit starts
#          anything but `~/.local/bin/torroworkplace daemon` or has another KillMode (a drop-in), it stops here,
#          before anything is stopped or installed; else `enable` when it is not. Then the programs to
#          ~/.local/bin, the .desktop files and SVG icons from packaging/linux/ to ~/.local/share, `restart`, and a
#          check that a new service process runs the file just installed.
# macOS:   the bundles of scripts/macos-dev-bundles.sh, made from target/release, to ~/Applications (or
#          `macos-dest`). The service starts at login through the LaunchAgent the setting "Start at login"
#          writes: when it is missing or starts another program, `torroworkplace login-item enable` of the new
#          service switches the setting on, once no service runs any more. When launchd has an agent loaded whose
#          definition is not `<new service> daemon`, it is unloaded (`bootout`) first. Then the service is
#          restarted (`launchctl kickstart -k`, or `bootstrap` when the agent is not loaded), up to three times,
#          since macOS may kill the first start of a replaced program (OS_REASON_CODESIGNING), and checked: a new
#          process that runs the file just installed.
# Both:    every service of this user that holds the service's lock (`torroworkplace service-lock`): one an app
#          started, one that took over from another copy, launchd's own; it is ended first, and the new service is
#          started only once `torroworkplace service-lock free` finds the lock free twice, a pause apart. Apps still
#          running a replaced program of the suite are named afterwards by program, role and pid. With
#          --restart-apps they are ended with SIGTERM and started again: SIGTERM skips an app's own question
#          before closing, so input not saved in them is lost. Compose windows, reminders, MCP servers, the
#          service's helpers and processes whose arguments cannot be read are only named, never ended. MCP
#          configurations are not touched.
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
# A process is told by what it is, not by its command line. Its start time tells it apart from a later process
# with the same pid: every identity is read between two readings of the start time, and an identity whose start
# changed in between, or that misses a part, counts as none.

# Whether the process `pid` belongs to this user.
process_owned() {
    case "$os" in
        Linux) [ -O "$proc/$1" ] ;;
        *) [ "$(ps -o uid= -p "$1" 2>/dev/null | tr -d ' ' || true)" = "$uid" ] ;;
    esac
}

# When `pid` started, in the system's own form; only compared, never shown. Empty when it is gone.
process_start() {
    case "$os" in
        Linux) { cat "$proc/$1/stat" 2>/dev/null || true; } | awk '{ sub(/.*\) /, ""); print $20 }' ;;
        *) ps -o lstart= -p "$1" 2>/dev/null || true ;;
    esac
}

# The path `pid` runs: Linux the program file (/proc/<pid>/exe, " (deleted)" once replaced), macOS argv[0], which
# launchd and LaunchServices set to the program file. Empty when it cannot be read.
process_path() {
    case "$os" in
        Linux) readlink "$proc/$1/exe" 2>/dev/null || true ;;
        *) ps -o comm= -p "$1" 2>/dev/null || true ;;
    esac
}

# The arguments of `pid` after the program, one per line; fails when they cannot be read. Read to tell a role or
# to start the program again, never shown.
process_args() {
    local args comm
    case "$os" in
        Linux)
            # procfs reports size 0 for cmdline, so it is read, not sized up; empty means unreadable (a zombie).
            args="$({ tr '\0' '\n' < "$proc/$1/cmdline"; } 2>/dev/null)" || return 1
            [ -n "$args" ] || return 1
            printf '%s\n' "$args" | sed '1d'
            ;;
        *)
            args="$(ps -o args= -p "$1" 2>/dev/null || true)"
            comm="$(ps -o comm= -p "$1" 2>/dev/null || true)"
            [ -n "$args" ] && [ -n "$comm" ] || return 1
            # Without argv[0], which may hold blanks (a bundle path); the arguments of the suite hold none.
            local words=()
            read -r -a words <<< "${args#"$comm"}" || true
            for args in ${words[@]+"${words[@]}"}; do echo "$args"; done
            ;;
    esac
}

# The identity of the file at `path` (device and inode), empty when there is none.
file_id() {
    case "$os" in
        Linux) stat -L -c %d:%i "$1" 2>/dev/null || true ;;
        *) stat -L -f %d:%i "$1" 2>/dev/null || true ;;
    esac
}

# Whether `pid` runs the file `file` itself, not only one at the same path: Linux device and inode of
# /proc/<pid>/exe; macOS argv[0], and device and inode of one and the same program file lsof lists for it (any of
# them, the first is not always the program; an inode alone may repeat on another volume).
runs_file() {
    local pid="$1" file="$2" want dev ino
    want="$(file_id "$file")"
    [ -n "$want" ] || return 1
    case "$os" in
        Linux) [ "$(file_id "$proc/$pid/exe")" = "$want" ] ;;
        *)
            [ "$(process_path "$pid")" = "$file" ] || return 1
            # One line `<device> <inode>` per file of lsof's field output (`f` starts a file, `D` in hex).
            { lsof -a -p "$pid" -d txt -FfDi 2>/dev/null || true; } | awk '
                /^f/ { if (seen) print d, i; d = ""; i = ""; seen = 1; next }
                /^D/ { d = substr($0, 2) } /^i/ { i = substr($0, 2) }
                END { if (seen) print d, i }' | {
                while read -r dev ino; do
                    case "$dev" in 0x[0-9a-fA-F]* | [0-9]*) ;; *) continue ;; esac
                    [ "$((dev)):$ino" = "$want" ] && exit 0
                done
                exit 1
            }
            ;;
    esac
}

# The pids that hold a lock on the file `lock` (Linux, /proc/locks), one per line. The device there may differ
# from stat's on btrfs, so the inode is compared, together with the open file below.
lock_holders() {
    local ino
    ino="$(file_id "$1")"
    ino="${ino#*:}"
    [ -n "$ino" ] || return 0
    { cat "$proc/locks" 2>/dev/null || true; } | awk -v ino="$ino" '
        $2 != "->" { n = split($6, dev, ":"); if (n == 3 && dev[3] == ino) print $5 }'
}

# Whether `pid` holds the service's lock `lock`. Linux: one of its descriptors is the lock file itself (device and
# inode after resolving links, so a state directory behind a symlink is no other file) and its fdinfo shows an
# exclusive flock taken by this pid (`lock: … FLOCK ADVISORY WRITE <pid>`; a waiting one is marked `->`). macOS:
# the file open (lsof shows no locks there); `service-lock free` decides afterwards.
holds_lock() {
    local pid="$1" lock="$2" fd want
    case "$os" in
        Linux)
            want="$(file_id "$lock")"
            [ -n "$want" ] || return 1
            for fd in "$proc/$pid/fd/"*; do
                [ "$(file_id "$fd")" = "$want" ] || continue
                awk -v pid="$pid" '$1 == "lock:" && $3 == "FLOCK" && $4 == "ADVISORY" && $5 == "WRITE" && $6 == pid {
                    found = 1 } END { exit !found }' "$proc/$pid/fdinfo/${fd##*/}" 2>/dev/null && return 0
            done
            return 1
            ;;
        *) { lsof -a -p "$pid" -t -- "$lock" 2>/dev/null || true; } | grep -qx "$pid" ;;
    esac
}

# Whether no service holds the lock, asked of the new program `program` (`service-lock free`): 0 free, 3 held,
# anything else unknown.
lock_free() {
    local rc=0
    "$1" service-lock free >/dev/null 2>&1 || rc=$?
    return "$rc"
}

# The identity of the service `pid` (`start|path`) when it is one of this user: its program is `torroworkplace`,
# its subcommand `daemon`, and it holds the service's lock `lock`, which only the service does. Fails
# otherwise, also when the pid changed hands while it was read.
service_id() {
    local pid="$1" lock="$2" start path
    start="$(process_start "$pid")"
    [ -n "$start" ] || return 1
    process_owned "$pid" || return 1
    path="$(process_path "$pid")"
    path="${path% (deleted)}"
    [ "${path##*/}" = torroworkplace ] || return 1
    [ "$( (process_args "$pid" || true) | sed -n 1p)" = daemon ] || return 1
    holds_lock "$pid" "$lock" || return 1
    [ "$(process_start "$pid")" = "$start" ] || return 1
    echo "$start|$path"
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

# Ends every service of this user that holds `lock`, whoever started it (an app, a takeover, launchd): it would
# make the new service give up as "already running". SIGTERM ends it like `systemctl --user stop` does; its
# identity is read again right before the signal, and a signal that does not reach a living process is an error.
# It succeeds only once the new program `program` finds the lock free twice, a pause apart: a service taking over
# from another one (`daemon --takeover`) asks for the lock every 250 ms and gets it once that one ended, so this
# looks again, up to five rounds. Only then may the new service be started.
end_services() {
    local lock="$1" program="$2" round=0 pid id ended rc held
    while [ "$round" -lt 5 ]; do
        round=$((round + 1))
        ended=""
        for pid in $(pgrep -U "$uid" -f torroworkplace 2>/dev/null || true); do
            id="$(service_id "$pid" "$lock")" || continue
            [ "$(service_id "$pid" "$lock" || true)" = "$id" ] || continue
            if ! kill "$pid" 2>/dev/null; then
                # Gone meanwhile is what was wanted; still there means the signal did not reach it.
                kill -0 "$pid" 2>/dev/null || continue
                warn "cannot end the service (pid $pid) that holds $lock"
                return 1
            fi
            ended="$ended $pid"
        done
        if [ -n "$ended" ]; then
            say "ended the running service (pid${ended})"
            # shellcheck disable=SC2086 # one pid per word
            if ! wait_gone $ended; then
                warn "the old service (pid${ended}) did not end"
                return 1
            fi
        fi
        rc=0
        lock_free "$program" || rc=$?
        if [ "$rc" = 0 ]; then
            sleep "$pause"
            rc=0
            lock_free "$program" || rc=$?
            [ "$rc" = 0 ] && return 0
        fi
        if [ "$rc" != 3 ]; then
            warn "cannot tell whether a service holds $lock ($program service-lock free: $rc)"
            return 1
        fi
        sleep "$pause"
    done
    held=""
    [ "$os" != Linux ] || held="$(lock_holders "$lock" | tr '\n' ' ')"
    warn "a service still holds $lock${held:+ (pid ${held% })}"
    return 1
}

# The lock the service holds, from the new program `program`; empty with a warning when it cannot tell.
service_lock() {
    "$1" service-lock 2>/dev/null || warn "cannot ask $1 for the service's lock"
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

# Whether the loaded unit, drop-ins included, runs exactly `bin/torroworkplace daemon` as its one ExecStart and
# stops with KillMode=process; says what differs.
unit_as_wanted() {
    local bin="$1" exec kill
    # The property as D-Bus has it: the number of commands, then per command its path, the number of arguments
    # and each argument quoted, so no argument can pass for a separator.
    exec="$(busctl --user get-property org.freedesktop.systemd1 \
        /org/freedesktop/systemd1/unit/torroworkplace_2eservice org.freedesktop.systemd1.Service ExecStart \
        2>/dev/null || true)"
    kill="$(systemctl --user show -p KillMode --value "$unit" 2>/dev/null || true)"
    case "$exec" in
        "a(sasbttttuii) 1 \"$bin/torroworkplace\" 2 \"$bin/torroworkplace\" \"daemon\" "*) ;;
        *) warn "$unit does not start exactly '$bin/torroworkplace daemon' (ExecStart: ${exec:-unknown})"; return 1 ;;
    esac
    if [ "$kill" != process ]; then
        warn "$unit has KillMode=$kill, so a stop would end the apps the service started"
        return 1
    fi
    return 0
}

# Sets the unit up before the old service stops: written, loaded, checked, enabled. Fails when a drop-in or
# another unit of the same name makes it differ, naming the drop-ins.
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
    if ! unit_as_wanted "$bin"; then
        warn "drop-ins: $(systemctl --user show -p DropInPaths --value "$unit" 2>/dev/null || true)" \
            "(see: systemctl --user cat $unit). Nothing was stopped or installed."
        return 1
    fi
    if ! systemctl --user is-enabled --quiet "$unit" 2>/dev/null; then
        say "enabling $unit"
        if ! systemctl --user enable "$unit"; then
            warn "systemctl --user enable $unit failed; nothing was installed"
            return 1
        fi
    fi
    return 0
}

# Whether the unit runs the file `program` in a process other than `old` and still does a pause later; prints
# the pid.
unit_runs() {
    local program="$1" old="${2:-}" tries=0 pid
    while [ "$tries" -lt 5 ]; do
        tries=$((tries + 1))
        pid="$(unit_pid)"
        if [ -n "$pid" ] && [ "$pid" != "$old" ] && runs_file "$pid" "$program"; then
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
    local bin="$1" old="${2:-}" pid out lock
    lock="$(service_lock "$bin/torroworkplace")"
    if [ -z "$lock" ] || ! end_services "$lock" "$bin/torroworkplace"; then
        warn "the service was not restarted"
        status=1
        return 0
    fi
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
        systemctl --user stop "$unit" 2>/dev/null || warn "systemctl --user stop $unit failed; installing anyway"
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

# Processes of this user still running a replaced program of the suite from `bin`: `pid<TAB>program<TAB>id`
# lines, `id` being `start|path`.
old_processes_linux() {
    local bin="$1" link pid target program start
    for link in "$proc"/[0-9]*/exe; do
        pid="${link%/exe}"
        pid="${pid##*/}"
        start="$(process_start "$pid")"
        target="$(readlink "$link" 2>/dev/null)" || continue
        case "$target" in
            "$bin/"*" (deleted)") ;;
            *) continue ;;
        esac
        program="${target#"$bin/"}"
        program="${program% (deleted)}"
        is_program "$program" || continue
        process_owned "$pid" || continue
        [ -n "$start" ] && [ "$(process_start "$pid")" = "$start" ] || continue
        printf '%s\t%s\t%s\n' "$pid" "$program" "$start|$target"
    done
    return 0
}

# Sends SIGTERM to the app `pid` only when it still is the process `id` (`start|path`) of this user.
end_app() {
    local pid="$1" id="$2"
    process_owned "$pid" || return 1
    [ "$(process_start "$pid")|$(process_path "$pid")" = "$id" ] || return 1
    kill "$pid" 2>/dev/null
}

# Ends the app `pid` (still the process `id`) running `program` from `bin` and starts it again with the same
# arguments, detached from this shell.
restart_app_linux() {
    local pid="$1" program="$2" bin="$3" id="$4" arg
    local args=()
    # Read before the process ends; a newline inside an argument would split it, which the suite's never hold.
    while IFS= read -r arg; do
        args+=("$arg")
    done <<< "$(process_args "$pid" || true)"
    [ -n "${args[0]:-}" ] || args=()
    end_app "$pid" "$id" || { warn "$program (pid $pid) ended or changed meanwhile; not restarted"; return 0; }
    wait_gone "$pid" || { warn "$program (pid $pid) did not end; restart it yourself"; return 0; }
    (cd "$HOME" && setsid "$bin/$program" ${args[@]+"${args[@]}"} >/dev/null 2>&1 < /dev/null &)
    say "restarted $program"
}

# --- macOS -----------------------------------------------------------------------------------------------------

# The pid launchd runs for `target` (gui/<uid>/<label>), empty when the agent is not loaded or not running.
launchd_pid() {
    { launchctl print "$1" 2>/dev/null || true; } | awk '$1 == "pid" && $2 == "=" { print $3; exit }'
}

# The definition launchd has loaded for `target`, each part marked: `program <path>`, `arg <argument>` per
# argument and `end` once the argument block closed. A missing program cannot hide behind an argument.
launchd_definition() {
    { launchctl print "$1" 2>/dev/null || true; } | awk '
        args && /^[[:space:]]*\}/ { args = 0; print "end"; next }
        args { sub(/^[[:space:]]*/, ""); print "arg " $0; next }
        /^[[:space:]]*program = / { sub(/^[[:space:]]*program = /, ""); print "program " $0; next }
        /^[[:space:]]*arguments = \{/ { args = 1 }'
}

# Whether launchd has `target` loaded: prints `loaded`, `unloaded` (launchctl print: 113, no such service) or
# `error` (any other failure, which tells nothing).
agent_state() {
    local rc=0
    launchctl print "$1" >/dev/null 2>&1 || rc=$?
    case "$rc" in
        0) echo loaded ;;
        113) echo unloaded ;;
        *) echo error ;;
    esac
}

# Whether launchd runs the file `program` for `target` in a process other than `old`, and still does a pause
# later; prints the pid.
launchd_runs() {
    local target="$1" program="$2" old="${3:-}" tries=0 pid
    while [ "$tries" -lt 5 ]; do
        tries=$((tries + 1))
        sleep "$pause"
        pid="$(launchd_pid "$target")"
        [ -n "$pid" ] && [ "$pid" != "$old" ] || continue
        runs_file "$pid" "$program" || continue
        sleep "$pause"
        if [ "$(launchd_pid "$target")" = "$pid" ]; then
            echo "$pid"
            return 0
        fi
    done
    return 1
}

# Unloads the agent `target` and waits until launchd has let it go; on failure says why. Only `unloaded` counts:
# a query that fails proves nothing.
unload_agent() {
    local target="$1" out rc=0 tries=0 state
    out="$(launchctl bootout "$target" 2>&1)" || rc=$?
    while :; do
        state="$(agent_state "$target")"
        [ "$state" = unloaded ] && return 0
        tries=$((tries + 1))
        if [ "$state" = error ] || [ "$tries" -ge 5 ]; then
            warn "launchctl bootout $target ($rc${out:+: $out}) did not unload the old agent (state: $state);" \
                "the service was not restarted"
            return 1
        fi
        sleep "$pause"
    done
}

# Makes sure the service's LaunchAgent starts `svc`, the service in the new bundles, and (re)starts it.
restart_service_macos() {
    local svc="$1" line state plist label domain target loaded=0 stale=0 attempt pid old reason out rc lock wanted
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
    lock="$(service_lock "$svc")"
    # What launchd has loaded may differ from the file (an earlier unload failed, the file was replaced by hand, a
    # definition it cannot show): checked on every run, as a whole.
    wanted="$(printf 'program %s\narg %s\narg daemon\nend' "$svc" "$svc")"
    case "$(agent_state "$target")" in
        loaded)
            loaded=1
            [ "$(launchd_definition "$target")" = "$wanted" ] || stale=1
            ;;
        error)
            warn "cannot ask launchd about $target (launchctl print); the service was not restarted"
            status=1
            return 0
            ;;
    esac
    if [ "$state" != current ] || [ "$stale" = 1 ]; then
        if [ "$loaded" = 1 ]; then
            [ "$stale" = 0 ] || say "launchd has the agent loaded with another definition; loading it again"
            unload_agent "$target" || { status=1; return 0; }
        fi
        # The setting changes only once the lock is shown free; `login-item enable` then holds it itself while it
        # changes the setting, so no service starts and writes settings.json meanwhile.
        if [ -z "$lock" ] || ! end_services "$lock" "$svc"; then
            warn "cannot make sure no service runs; \"Start at login\" was not changed, the service not restarted"
            status=1
            return 0
        fi
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
    # Otherwise launchd's own service ends here too, so the lock is shown free before kickstart starts the new
    # one: a takeover successor waiting for it cannot slip in between.
    elif [ -z "$lock" ] || ! end_services "$lock" "$svc"; then
        warn "the service was not restarted"
        status=1
        return 0
    fi
    attempt=1
    while [ "$attempt" -le 3 ]; do
        old="$(launchd_pid "$target")"
        rc=0
        case "$(agent_state "$target")" in
            loaded) out="$(launchctl kickstart -k "$target" 2>&1)" || rc=$? ;;
            unloaded) out="$(launchctl bootstrap "$domain" "$plist" 2>&1)" || rc=$? ;;
            *) out="launchctl print $target failed"; rc=1 ;;
        esac
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

# The processes of this user whose program lies inside `dest`, as `pid start|path` lines, taken before the
# install: what runs from the bundles now runs the old programs afterwards.
processes_in() {
    local dest="$1" pid comm start
    { ps -axo pid=,comm= 2>/dev/null || true; } | while read -r pid comm; do
        case "$comm" in
            "$dest/"*) ;;
            *) continue ;;
        esac
        start="$(process_start "$pid")"
        process_owned "$pid" || continue
        [ -n "$start" ] && [ "$(process_path "$pid")" = "$comm" ] && [ "$(process_start "$pid")" = "$start" ] \
            && echo "$pid $start|$comm"
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
        [ "$(process_start "$pid")|$(process_path "$pid")" = "$id" ] && printf '%s\t%s\t%s\n' "$pid" "$exe" "$id"
    done <<< "$before"
    return 0
}

# Ends the app `pid` (still the process `id`) whose program is `path` and opens its bundle again.
restart_app_macos() {
    local pid="$1" path="$2" id="$3" bundle
    bundle="${path%%.app/Contents/MacOS/*}.app"
    end_app "$pid" "$id" || { warn "${bundle##*/} (pid $pid) ended or changed meanwhile; not restarted"; return 0; }
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
# what else it is, which is only named. Told by every argument the suite takes for it, and `unknown` when the
# arguments cannot be read.
process_role() {
    local pid="$1" program="$2" args sub
    case "$program" in
        *"/Contents/Library/LoginItems/"*) echo service; return 0 ;;
        */torro-reminder) echo reminder; return 0 ;;
    esac
    args="$(process_args "$pid")" || { echo unknown; return 0; }
    # torroinbox --compose (also after --view mail), torrocalendar --reminders (crates/torroinbox/src/compose.rs,
    # crates/torroworkplace/src/reminders.rs).
    if printf '%s\n' "$args" | grep -qx -- --compose; then
        echo "compose window"
        return 0
    fi
    # The calendar's reminder window also opens with `--open reminder…` or `--open=reminder…` (its demo states,
    # crates/torrocalendar/src/reminder.rs).
    if printf '%s\n' "$args" | grep -qx -e --reminders -e '--open=reminder.*' \
        || printf '%s\n' "$args" | awk 'prev == "--open" && /^reminder/ { found = 1 } { prev = $0 }
            END { exit !found }'; then
        echo reminder
        return 0
    fi
    sub="$(printf '%s\n' "$args" | sed -n 1p)"
    case "${program##*/}:$sub" in
        torroworkplace:daemon) echo service ;;
        torroworkplace:mcp) echo "MCP server" ;;
        torroworkplace: | torroworkplace:ui) echo app ;;
        torroworkplace:*) echo "service helper" ;;
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
    [ -z "$apps" ] || warn "still running the old program: ${apps#, }; restart them, or run again with" \
        "--restart-apps (SIGTERM: input not saved in them is lost)"
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
            # Absolute, since processes and launchd name absolute paths.
            dest="${dest:-$HOME/Applications}"
            mkdir -p "$dest"
            dest="$(cd "$dest" && pwd -P)"
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
