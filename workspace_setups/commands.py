from __future__ import annotations

import json
import os
import re
import shlex
import shutil
from pathlib import Path
from typing import Any, Iterable
from urllib.parse import unquote, urlparse

PLACEHOLDER = re.compile(r"%[fFuUdDnNickvm]")
SECRET = re.compile(r"(?i)(token|password|passwd|secret|api[-_]?key|auth)")
UNSAFE_FLAGS = {"--remote-debugging-port", "--user-data-dir", "--temp-profile", "--crashpad-handler-pid", "--type", "--utility-sub-type"}
SHELLS = {"sh", "bash", "zsh", "fish", "dash"}
INTERPRETER_FLAGS = {
    "python": {"-c", "-m"}, "python3": {"-c", "-m"},
    "perl": {"-e", "-E"}, "ruby": {"-e"}, "node": {"-e", "--eval"},
}


def sanitize_command(arguments: Iterable[str]) -> list[str]:
    args = list(arguments)
    if not args:
        return []
    if any(character.isspace() for character in args[0]) and not os.path.exists(args[0]):
        try:
            expanded = shlex.split(args[0])
        except ValueError:
            return []
        if not expanded:
            return []
        args = [*expanded, *args[1:]]
    clean = [args[0]]
    skip = False
    for arg in args[1:]:
        if skip:
            skip = False
            continue
        key = arg.split("=", 1)[0]
        if key in UNSAFE_FLAGS or SECRET.search(key):
            skip = "=" not in arg
            continue
        if PLACEHOLDER.search(arg) or "\0" in arg or arg.startswith("/tmp/"):
            continue
        clean.append(arg)
    return clean


def has_embedded_shell(arguments: Iterable[str]) -> bool:
    args = list(arguments)
    for index, arg in enumerate(args):
        executable = Path(arg).name
        trailing = set(args[index + 1:])
        if executable in SHELLS and any(flag.startswith("-") and "c" in flag[1:] for flag in trailing):
            return True
        family = executable.rstrip("0123456789.")
        if family in INTERPRETER_FLAGS and trailing & INTERPRETER_FLAGS[family]:
            return True
    return False


def proc_command(pid: int, proc: Path = Path("/proc")) -> list[str]:
    try:
        data = (proc / str(pid) / "cmdline").read_bytes()
    except (OSError, ValueError):
        return []
    return sanitize_command(part.decode(errors="replace") for part in data.split(b"\0") if part)


def proc_cwd(pid: int, proc: Path = Path("/proc")) -> str | None:
    try:
        path = os.readlink(proc / str(pid) / "cwd")
        return path if os.path.isabs(path) and not path.startswith(("/proc/", "/tmp/")) else None
    except OSError:
        return None


def descendant_cwd(pid: int, proc: Path = Path("/proc")) -> str | None:
    """Prefer a terminal's deepest child, which is normally its active shell."""
    descendants = {pid: 0}
    changed = True
    while changed:
        changed = False
        try:
            entries = list(proc.iterdir())
        except OSError:
            break
        for entry in entries:
            if not entry.name.isdigit() or int(entry.name) in descendants:
                continue
            try:
                status = (entry / "status").read_text(errors="replace").splitlines()
                parent = int(next(line.split()[1] for line in status if line.startswith("PPid:")))
            except (OSError, StopIteration, ValueError):
                continue
            if parent in descendants:
                descendants[int(entry.name)] = descendants[parent] + 1
                changed = True
    for child in sorted(descendants, key=lambda item: (descendants[item], item), reverse=True):
        cwd = proc_cwd(child, proc)
        if cwd:
            return cwd
    return None


def vscode_workspace(title: str, env: dict[str, str] | None = None) -> str | None:
    """Match a VS Code window title to Code's persisted folder/window state."""
    env = os.environ if env is None else env
    home = Path(env.get("HOME", "~")).expanduser()
    config_home = Path(env.get("XDG_CONFIG_HOME", home / ".config"))
    candidates: list[str] = []
    for product in ("Code", "Code - OSS", "VSCodium"):
        path = config_home / product / "User/globalStorage/storage.json"
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        state = data.get("windowsState") or {}
        entries = [state.get("lastActiveWindow") or {}, *(state.get("openedWindows") or [])]
        backup = data.get("backupWorkspaces") or {}
        entries.extend(backup.get("folders") or [])
        entries.extend(backup.get("workspaces") or [])
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            raw = entry.get("folder") or entry.get("folderUri") or entry.get("workspace") or entry.get("configPath")
            if isinstance(raw, dict):
                raw = raw.get("configPath") or raw.get("path")
            if not isinstance(raw, str) or not raw:
                continue
            parsed = urlparse(raw)
            value = unquote(parsed.path) if parsed.scheme == "file" else raw
            if os.path.isabs(value) and value not in candidates:
                candidates.append(value)
    folded_title = title.casefold()
    matches = [
        path for path in candidates
        if (Path(path).stem if path.endswith(".code-workspace") else Path(path).name).casefold() in folded_title
    ]
    return max(matches, key=len) if matches else None


def desktop_entries(env: dict[str, str] | None = None) -> list[Path]:
    env = os.environ if env is None else env
    home = Path(env.get("HOME", "~")).expanduser()
    data_home = Path(env.get("XDG_DATA_HOME", home / ".local/share"))
    data_dirs = [Path(item) for item in env.get("XDG_DATA_DIRS", "/usr/local/share:/usr/share").split(":") if item]
    return [path for root in [data_home, *data_dirs] for path in (root / "applications").glob("*.desktop")]


def desktop_command(wm_class: str, paths: Iterable[Path] | None = None) -> list[str]:
    target = wm_class.casefold()
    fallback: list[str] = []
    for path in paths if paths is not None else desktop_entries():
        try:
            fields = {}
            for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
                if "=" in line and not line.startswith((" ", "#")):
                    key, value = line.split("=", 1)
                    fields.setdefault(key, value)
            command = sanitize_command(shlex.split(fields.get("Exec", "")))
        except (OSError, ValueError):
            continue
        if not command:
            continue
        executable = Path(command[0]).name.casefold()
        if fields.get("StartupWMClass", "").casefold() == target:
            return command
        if target in {path.stem.casefold(), executable}:
            fallback = command
    return fallback


def resolve_command(client: dict[str, Any], proc: Path = Path("/proc"), entries: Iterable[Path] | None = None) -> list[str]:
    pid = int(client.get("pid") or 0)
    command = proc_command(pid, proc) if pid else []
    wm_class = str(client.get("initialClass") or client.get("class") or "")
    desktop = desktop_command(wm_class, entries)
    lowered = wm_class.casefold()
    # Desktop entries avoid persisting renderer/helper command lines from
    # multi-process applications such as Chromium and VS Code.
    candidate = desktop or command
    if has_embedded_shell(candidate):
        return []
    if candidate:
        adapted = adapt_command(client, candidate, proc)
        if adapted and _executable_available(adapted[0]):
            return adapted
    return []


def adapt_command(client: dict[str, Any], command: list[str], proc: Path = Path("/proc")) -> list[str]:
    application = str(client.get("initialClass") or client.get("class") or Path(command[0]).name).casefold()
    if "foot" in application:
        cwd = descendant_cwd(int(client.get("pid") or 0), proc)
        base = [command[0]]
        return base + (["--working-directory", cwd] if cwd else [])
    if application in {"code", "code-oss", "visual studio code"} or "vscode" in application:
        for arg in command[1:]:
            if (arg.endswith((".code-workspace",)) or os.path.isdir(arg)) and os.path.isabs(arg):
                return [command[0], "--new-window", arg]
        workspace = vscode_workspace(str(client.get("title", "")))
        if workspace:
            return [command[0], "--new-window", workspace]
        return [command[0], "--new-window"]
    if "chrom" in application:
        app_arguments = [
            argument for argument in command[1:]
            if argument.startswith(("--app=", "--app-id=", "--class=", "--profile-directory="))
        ]
        return [command[0], *app_arguments]
    return sanitize_command(command)


def _executable_available(executable: str) -> bool:
    if os.path.isabs(executable):
        return os.path.isfile(executable) and os.access(executable, os.X_OK)
    return shutil.which(executable) is not None
