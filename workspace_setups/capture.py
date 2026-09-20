from __future__ import annotations

from typing import Any

from .commands import resolve_command
from .hyprland import Hyprland, normal_clients
from .layout import infer_tree
from .storage import now, slugify


def capture(name: str, hyprland: Hyprland, existing: dict[str, Any] | None = None) -> dict[str, Any]:
    monitors_raw = hyprland.query("monitors")
    workspaces_raw = hyprland.query("workspaces")
    clients = normal_clients(hyprland.query("clients"))
    active_workspace = hyprland.query("activeworkspace")
    timestamp = now()
    monitors = [{key: monitor.get(key) for key in ("name", "width", "height", "scale", "x", "y")} for monitor in monitors_raw]
    monitor_names = {monitor.get("id", index): monitor.get("name", "") for index, monitor in enumerate(monitors_raw)}
    windows = []
    for index, client in enumerate(clients, 1):
        at, size = client.get("at", [0, 0]), client.get("size", [0, 0])
        command = resolve_command(client)
        fullscreen = int(client.get("fullscreen") or 0)
        window = {
            "id": f"window-{index}", "application": client.get("initialClass") or client.get("class"),
            "command": command, "restorable": bool(command), "initialClass": client.get("initialClass", ""),
            "class": client.get("class", ""), "titleHint": client.get("title") or client.get("initialTitle", ""),
            "workspace": str((client.get("workspace") or {}).get("name", "")),
            "monitor": monitor_names.get(client.get("monitor"), client.get("monitor", "")),
            "floating": bool(client.get("floating")), "fullscreen": fullscreen > 0,
            "fullscreenMode": "maximized" if fullscreen == 1 else "fullscreen",
            "pinned": bool(client.get("pinned")),
            "geometry": {"x": at[0], "y": at[1], "width": size[0], "height": size[1]},
        }
        windows.append(window)
    workspaces = []
    for workspace in workspaces_raw:
        workspace_windows = [item for item in windows if item["workspace"] == str(workspace.get("name", ""))]
        workspaces.append({
            "id": workspace.get("id"), "name": str(workspace.get("name", workspace.get("id"))),
            "monitor": workspace.get("monitor", ""), "layout": "dwindle", "tree": infer_tree(workspace_windows),
        })
    return {
        "schemaVersion": 1, "id": existing["id"] if existing else slugify(name), "name": name,
        "createdAt": existing.get("createdAt", timestamp) if existing else timestamp, "updatedAt": timestamp,
        "environment": {"hyprlandLayout": "dwindle", "monitors": monitors, "activeWorkspace": active_workspace.get("name")},
        "workspaces": workspaces, "windows": windows,
    }
