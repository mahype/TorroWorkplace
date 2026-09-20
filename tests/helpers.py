from __future__ import annotations

from pathlib import Path
from typing import Any

from workspace_setups.paths import Paths
from workspace_setups.storage import Store


def paths(root: Path) -> Paths:
    return Paths(root / "config", root / "data", root / "state")


def store(root: Path) -> Store:
    return Store(paths(root))


def profile(name: str = "Test") -> dict[str, Any]:
    return {
        "schemaVersion": 1, "id": name.lower(), "name": name,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z",
        "environment": {"hyprlandLayout": "dwindle", "monitors": [
            {"name": "DP-1", "width": 1920, "height": 1080, "x": 0, "y": 0, "scale": 1.0}
        ]},
        "workspaces": [{"id": 1, "name": "1", "monitor": "DP-1", "layout": "dwindle", "tree": {"window": "window-1"}}],
        "windows": [{
            "id": "window-1", "application": "foot", "command": ["foot"], "initialClass": "foot", "class": "foot",
            "titleHint": "Terminal", "workspace": "1", "monitor": "DP-1", "floating": False,
            "fullscreen": False, "pinned": False, "geometry": {"x": 0, "y": 0, "width": 900, "height": 700},
        }],
    }


class FakeHyprland:
    def __init__(self, clients: list[dict[str, Any]] | None = None):
        self.clients = clients or []
        self.dispatched: list[tuple[str, str]] = []
        self.monitors = [{"name": "DP-1", "width": 1920, "height": 1080, "x": 0, "y": 0, "scale": 1.0}]

    def query(self, resource: str) -> Any:
        if resource == "clients":
            return self.clients
        if resource == "monitors":
            return self.monitors
        if resource == "workspaces":
            return [{"id": 1, "name": "1", "monitor": "DP-1"}]
        if resource == "activeworkspace":
            return {"id": 1, "name": "1"}
        raise AssertionError(resource)

    def dispatch(self, expression: str) -> None:
        self.dispatched.append(("dispatch", expression))

    def move_workspace(self, workspace: str, monitor: str) -> None:
        self.dispatched.append(("move_workspace", f"{workspace}:{monitor}"))

    def focus_workspace(self, workspace: str) -> None:
        self.dispatched.append(("focus_workspace", workspace))

    def focus_monitor(self, monitor: str) -> None:
        self.dispatched.append(("focus_monitor", monitor))

    def move_window(self, address: str, *, workspace: str | None = None,
                    x: int | None = None, y: int | None = None) -> None:
        argument = workspace if workspace is not None else f"{x},{y}"
        self.dispatched.append(("move_window", f"{address}:{argument}"))

    def focus_window(self, address: str) -> None:
        self.dispatched.append(("focus_window", address))

    def set_floating(self, address: str, enabled: bool) -> None:
        self.dispatched.append(("set_floating", f"{address}:{enabled}"))

    def resize_window(self, address: str, width: int, height: int) -> None:
        self.dispatched.append(("resize_window", f"{address}:{width}x{height}"))

    def set_fullscreen(self, address: str, mode: str = "fullscreen") -> None:
        self.dispatched.append(("set_fullscreen", f"{address}:{mode}"))

    def pin_window(self, address: str) -> None:
        self.dispatched.append(("pin_window", address))

    def close_window(self, address: str) -> None:
        self.dispatched.append(("close_window", address))
        self.clients = [item for item in self.clients if item.get("address") != address]

    def layout(self, message: str) -> None:
        self.dispatched.append(("layout", message))

    def event_socket(self, timeout: float = 1.0) -> Any:
        raise OSError("test has no socket")
