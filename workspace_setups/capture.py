from __future__ import annotations

from typing import Any

from .browser import attach_tabs
from .commands import default_strategy, resolve_command
from .hyprland import Hyprland, normal_clients
from .layout import infer_tree
from .storage import SCHEMA_VERSION, StorageError, now, slugify

# Transient system dialogs that belong to another application's request.
IGNORED_CLASSES = ("xdg-desktop-portal", "polkit", "org.omarchy.polkit")


def capture(name: str, hyprland: Hyprland, existing: dict[str, Any] | None = None) -> dict[str, Any]:
    """Snapshot the focused workspace as a template.

    Floating geometry is stored relative to the monitor so the template can be
    restored on any workspace. Settings of an existing template are kept.
    """
    active = hyprland.query("activeworkspace")
    workspace_id = active.get("id")
    if workspace_id is None:
        raise StorageError("no active workspace")
    monitors = hyprland.query("monitors")
    monitor = next((item for item in monitors if item.get("name") == active.get("monitor")), monitors[0] if monitors else {})
    origin_x, origin_y = monitor.get("x", 0), monitor.get("y", 0)
    clients = [client for client in normal_clients(hyprland.query("clients"))
               if (client.get("workspace") or {}).get("id") == workspace_id
               and not str(client.get("class", "")).casefold().startswith(IGNORED_CLASSES)]
    previous = {window.get("titleHint"): window.get("strategy") for window in (existing or {}).get("windows", [])}
    timestamp = now()
    windows = []
    for index, client in enumerate(clients, 1):
        at, size = client.get("at", [0, 0]), client.get("size", [0, 0])
        command = resolve_command(client)
        fullscreen = int(client.get("fullscreen") or 0)
        window = {
            "id": f"window-{index}", "application": client.get("initialClass") or client.get("class"),
            "command": command, "restorable": bool(command), "initialClass": client.get("initialClass", ""),
            "class": client.get("class", ""), "titleHint": client.get("title") or client.get("initialTitle", ""),
            "floating": bool(client.get("floating")), "fullscreen": fullscreen > 0,
            "fullscreenMode": "maximized" if fullscreen == 1 else "fullscreen",
            "pinned": bool(client.get("pinned")),
            "geometry": {"x": at[0] - origin_x, "y": at[1] - origin_y, "width": size[0], "height": size[1]},
        }
        window["strategy"] = previous.get(window["titleHint"]) or default_strategy(window)
        windows.append(window)
    attach_tabs(windows)
    workspace_name = str(active.get("name", workspace_id))
    template = {
        "schemaVersion": SCHEMA_VERSION, "id": existing["id"] if existing else slugify(name), "name": name,
        "createdAt": existing.get("createdAt", timestamp) if existing else timestamp, "updatedAt": timestamp,
        "source": {
            "workspace": workspace_name, "monitor": monitor.get("name", ""),
            "monitorSize": {"width": monitor.get("width"), "height": monitor.get("height"), "scale": monitor.get("scale")},
        },
        "shortcut": (existing or {}).get("shortcut"),
        "restoreOnStartup": bool((existing or {}).get("restoreOnStartup", False)),
        "startupWorkspace": (existing or {}).get("startupWorkspace")
        or (workspace_name if workspace_name.isdigit() and workspace_name != "0" else None),
        "tree": infer_tree(windows), "windows": windows,
    }
    return template
