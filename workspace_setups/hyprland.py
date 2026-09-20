from __future__ import annotations

import json
import os
import socket
import subprocess
from pathlib import Path
from typing import Any, Callable


class HyprlandError(RuntimeError):
    pass


class Hyprland:
    def __init__(self, run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run, env: dict[str, str] | None = None):
        self.run = run
        self.env = os.environ if env is None else env

    def query(self, resource: str) -> Any:
        try:
            result = self.run(["hyprctl", "-j", resource], check=True, text=True, capture_output=True, timeout=5)
            return json.loads(result.stdout)
        except (OSError, subprocess.SubprocessError, json.JSONDecodeError) as error:
            raise HyprlandError(f"hyprctl {resource} failed: {error}") from error

    def dispatch(self, expression: str) -> None:
        """Run one dispatcher expression through Omarchy's Lua config provider."""
        command = ["hyprctl", "dispatch", expression]
        try:
            self.run(command, check=True, text=True, capture_output=True, timeout=5)
        except (OSError, subprocess.SubprocessError) as error:
            raise HyprlandError(f"Hyprland dispatch failed: {expression}") from error

    def move_workspace(self, workspace: str, monitor: str) -> None:
        self.dispatch(
            f"hl.dsp.workspace.move({{ workspace = {_lua_string(workspace)}, monitor = {_lua_string(monitor)} }})"
        )

    def focus_workspace(self, workspace: str) -> None:
        self.dispatch(f"hl.dsp.focus({{ workspace = {_lua_string(workspace)} }})")

    def focus_monitor(self, monitor: str) -> None:
        self.dispatch(f"hl.dsp.focus({{ monitor = {_lua_string(monitor)} }})")

    def move_window(self, address: str, *, workspace: str | None = None,
                    x: int | None = None, y: int | None = None) -> None:
        fields = [f"window = {_lua_string(window_selector(address))}"]
        if workspace is not None:
            fields.extend((f"workspace = {_lua_string(workspace)}", "follow = false"))
        if x is not None and y is not None:
            fields.extend((f"x = {int(x)}", f"y = {int(y)}", "relative = false"))
        self.dispatch(f"hl.dsp.window.move({{ {', '.join(fields)} }})")

    def focus_window(self, address: str) -> None:
        self.dispatch(f"hl.dsp.focus({{ window = {_lua_string(window_selector(address))} }})")

    def set_floating(self, address: str, enabled: bool) -> None:
        action = "enable" if enabled else "disable"
        self.dispatch(
            f"hl.dsp.window.float({{ window = {_lua_string(window_selector(address))}, action = {_lua_string(action)} }})"
        )

    def resize_window(self, address: str, width: int, height: int) -> None:
        self.dispatch(
            "hl.dsp.window.resize({ "
            f"window = {_lua_string(window_selector(address))}, x = {int(width)}, y = {int(height)}, relative = false"
            " })"
        )

    def set_fullscreen(self, address: str, mode: str = "fullscreen") -> None:
        if mode not in {"fullscreen", "maximized"}:
            raise HyprlandError(f"invalid fullscreen mode: {mode}")
        self.dispatch(
            "hl.dsp.window.fullscreen({ "
            f"window = {_lua_string(window_selector(address))}, mode = {_lua_string(mode)}, action = \"set\""
            " })"
        )

    def pin_window(self, address: str) -> None:
        self.dispatch(
            f"hl.dsp.window.pin({{ window = {_lua_string(window_selector(address))}, action = \"enable\" }})"
        )

    def close_window(self, address: str) -> None:
        self.dispatch(f"hl.dsp.window.close({{ window = {_lua_string(window_selector(address))} }})")

    def layout(self, message: str) -> None:
        self.dispatch(f"hl.dsp.layout({_lua_string(message)})")

    def event_socket(self, timeout: float = 1.0) -> socket.socket:
        signature = self.env.get("HYPRLAND_INSTANCE_SIGNATURE")
        runtime = self.env.get("XDG_RUNTIME_DIR")
        if not signature or not runtime:
            raise HyprlandError("Hyprland event socket environment is unavailable")
        path = Path(runtime) / "hypr" / signature / ".socket2.sock"
        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        connection.settimeout(timeout)
        connection.connect(str(path))
        return connection


def window_selector(address: str) -> str:
    normalized = str(address).strip().lower()
    if normalized.startswith("address:"):
        normalized = normalized.removeprefix("address:")
    normalized = normalized.removeprefix("0x")
    if not normalized or any(character not in "0123456789abcdef" for character in normalized):
        raise HyprlandError(f"invalid window address: {address}")
    return f"address:0x{normalized}"


def _lua_string(value: str) -> str:
    escaped = []
    for character in str(value):
        if character == "\\":
            escaped.append("\\\\")
        elif character == '"':
            escaped.append('\\"')
        elif character == "\n":
            escaped.append("\\n")
        elif character == "\r":
            escaped.append("\\r")
        elif character == "\t":
            escaped.append("\\t")
        elif ord(character) < 32:
            escaped.append(f"\\{ord(character):03d}")
        else:
            escaped.append(character)
    return '"' + "".join(escaped) + '"'


def normal_clients(clients: list[dict[str, Any]]) -> list[dict[str, Any]]:
    result = []
    for client in clients:
        workspace = client.get("workspace") or {}
        workspace_name = str(workspace.get("name", ""))
        normal_workspace = workspace.get("id", 0) != 0 and not workspace_name.startswith("special:")
        if client.get("mapped", True) and not client.get("hidden", False) and normal_workspace and client.get("class"):
            result.append(client)
    return result
