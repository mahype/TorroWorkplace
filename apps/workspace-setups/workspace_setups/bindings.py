from __future__ import annotations

import os
from pathlib import Path

from .storage import StorageError

MARKER = "-- workspace-setups: template shortcuts"
PLUGIN_BINDINGS = Path(__file__).resolve().parents[1] / "hypr" / "bindings.lua"


def user_bindings(env: dict[str, str] | None = None) -> Path:
    env = os.environ if env is None else env
    home = Path(env.get("HOME", "~")).expanduser()
    return Path(env.get("XDG_CONFIG_HOME") or home / ".config") / "hypr" / "bindings.lua"


def include_block(source: Path = PLUGIN_BINDINGS, env: dict[str, str] | None = None) -> str:
    env = os.environ if env is None else env
    home = Path(env.get("HOME", "~")).expanduser()
    try:
        expression = '(os.getenv("HOME") or "") .. ' + _lua_string("/" + str(source.relative_to(home)))
    except ValueError:
        expression = _lua_string(str(source))
    # The existence check keeps Hyprland's config loading if the plugin is removed.
    return (
        f"{MARKER}\n"
        f"local workspace_setups_bindings = {expression}\n"
        "local workspace_setups_file = io.open(workspace_setups_bindings, \"r\")\n"
        "if workspace_setups_file then workspace_setups_file:close(); dofile(workspace_setups_bindings) end\n"
    )


def installed(target: Path | None = None) -> bool:
    target = target or user_bindings()
    try:
        return MARKER in target.read_text(encoding="utf-8")
    except OSError:
        return False


def install(target: Path | None = None, source: Path = PLUGIN_BINDINGS, env: dict[str, str] | None = None) -> bool:
    """Append the include block once; returns False when it was already there."""
    target = target or user_bindings(env)
    if installed(target):
        return False
    if target.is_symlink() and not target.resolve().exists():
        raise StorageError(f"cannot edit broken symbolic link: {target}")
    target.parent.mkdir(parents=True, exist_ok=True)
    current = target.read_text(encoding="utf-8") if target.exists() else ""
    separator = "" if current == "" or current.endswith("\n\n") else ("\n" if current.endswith("\n") else "\n\n")
    with target.open("a", encoding="utf-8") as handle:
        handle.write(separator + include_block(source, env))
    return True


def _lua_string(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'
