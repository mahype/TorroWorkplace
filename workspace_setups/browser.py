"""Read open tabs per window from a Chromium-family session file.

Chromium appends its session state as SNSS command records to
``<profile>/Sessions/Session_*``. Replaying the few commands that describe
windows, tabs and navigations yields the URLs of every open window, which are
matched to Hyprland windows by the title of their active tab.
"""
from __future__ import annotations

import os
import struct
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

# Browser classes and their configuration directory below XDG_CONFIG_HOME.
PROFILE_ROOTS = {
    "chromium": "chromium",
    "google-chrome": "google-chrome",
    "brave-browser": "BraveSoftware/Brave-Browser",
}
URL_SCHEMES = {"http", "https", "file"}
MAX_URL_LENGTH = 4096

# SNSS command ids (components/sessions/core/session_service_commands.cc).
SET_TAB_WINDOW, SET_TAB_INDEX, UPDATE_NAVIGATION, SELECTED_NAVIGATION = 0, 2, 6, 7
SELECTED_TAB, WINDOW_TYPE, TAB_CLOSED, WINDOW_CLOSED = 8, 9, 16, 17
NORMAL_WINDOW = 0


def browser_family(window: dict[str, Any]) -> str | None:
    """The browser a normal (non-app) window belongs to, if it is one we read."""
    command = window.get("command") or []
    if any(str(argument).startswith("--app") for argument in command[1:]):
        return None
    for key in ("initialClass", "class"):
        value = str(window.get(key, "")).casefold()
        if value in PROFILE_ROOTS:
            return value
    return None


def session_file(family: str, command: list[str], env: dict[str, str] | None = None) -> Path | None:
    env = os.environ if env is None else env
    home = Path(env.get("HOME", "~")).expanduser()
    config_home = Path(env.get("XDG_CONFIG_HOME", home / ".config"))
    profile = next((argument.split("=", 1)[1] for argument in command
                    if argument.startswith("--profile-directory=")), "Default")
    folder = config_home / PROFILE_ROOTS[family] / profile / "Sessions"
    try:
        candidates = [path for path in folder.glob("Session_*") if path.is_file()]
    except OSError:
        return None
    return max(candidates, key=lambda path: path.stat().st_mtime, default=None)


def _string(payload: bytes, offset: int, width: int) -> tuple[str, int]:
    (length,) = struct.unpack_from("<i", payload, offset)
    offset += 4
    size = length * width
    if length < 0 or offset + size > len(payload):
        raise ValueError("invalid string")
    raw = payload[offset:offset + size]
    text = raw.decode("utf-16-le" if width == 2 else "utf-8", errors="replace")
    return text, offset + ((size + 3) & ~3)


def read_session(data: bytes) -> list[dict[str, Any]]:
    """Replay an SNSS session log into ``[{"urls": [...], "title": str}]``."""
    if data[:4] != b"SNSS":
        return []
    tabs: dict[int, dict[str, Any]] = {}
    windows: dict[int, dict[str, Any]] = {}
    position = 8
    while position + 2 <= len(data):
        (size,) = struct.unpack_from("<H", data, position)
        position += 2
        if size == 0 or position + size > len(data):
            break
        command, payload = data[position], data[position + 1:position + size]
        position += size
        try:
            if command == SET_TAB_WINDOW:
                window, tab = struct.unpack_from("<ii", payload)
                tabs.setdefault(tab, {})["window"] = window
                windows.setdefault(window, {})
            elif command == SET_TAB_INDEX:
                tab, index = struct.unpack_from("<ii", payload)
                tabs.setdefault(tab, {})["index"] = index
            elif command == UPDATE_NAVIGATION:
                tab, index = struct.unpack_from("<ii", payload, 4)
                url, offset = _string(payload, 12, 1)
                title, _ = _string(payload, offset, 2)
                tabs.setdefault(tab, {}).setdefault("navigations", {})[index] = (url, title)
            elif command == SELECTED_NAVIGATION:
                tab, index = struct.unpack_from("<ii", payload)
                tabs.setdefault(tab, {})["selected"] = index
            elif command == SELECTED_TAB:
                window, index = struct.unpack_from("<ii", payload)
                windows.setdefault(window, {})["selected"] = index
            elif command == WINDOW_TYPE:
                window, kind = struct.unpack_from("<ii", payload)
                windows.setdefault(window, {})["type"] = kind
            elif command == TAB_CLOSED:
                tabs.pop(struct.unpack_from("<i", payload)[0], None)
            elif command == WINDOW_CLOSED:
                windows.pop(struct.unpack_from("<i", payload)[0], None)
        except (struct.error, ValueError):
            continue
    result = []
    for window_id, window in windows.items():
        if window.get("type", NORMAL_WINDOW) != NORMAL_WINDOW:
            continue
        members = sorted((tab.get("index", 0), tab_id) for tab_id, tab in tabs.items() if tab.get("window") == window_id)
        current = []
        for _, tab_id in members:
            tab = tabs[tab_id]
            navigations = tab.get("navigations") or {}
            entry = navigations.get(tab.get("selected", max(navigations, default=0)))
            if entry:
                current.append(entry)
        if not current:
            continue
        selected = min(max(int(window.get("selected", 0)), 0), len(current) - 1)
        result.append({"urls": [url for url, _ in current if restorable_url(url)], "title": current[selected][1]})
    return result


def restorable_url(url: str) -> bool:
    if not isinstance(url, str) or not url or len(url) > MAX_URL_LENGTH or any(ord(c) < 32 for c in url):
        return False
    return urlparse(url).scheme in URL_SCHEMES


def _window_title(title: str) -> str:
    # "Page title - Chromium" → "Page title"
    return title.rsplit(" - ", 1)[0].strip() if " - " in title else title.strip()


def attach_tabs(windows: list[dict[str, Any]], env: dict[str, str] | None = None) -> None:
    """Store the open tab URLs of every captured browser window in ``urls``."""
    sessions: dict[Path, list[dict[str, Any]]] = {}
    for window in windows:
        family = browser_family(window)
        if not family:
            continue
        path = session_file(family, window.get("command") or [], env)
        if path is None:
            continue
        if path not in sessions:
            try:
                sessions[path] = read_session(path.read_bytes())
            except OSError:
                sessions[path] = []
        title = _window_title(str(window.get("titleHint", ""))).casefold()
        candidates = sessions[path]
        match = next((item for item in candidates if item["title"].strip().casefold() == title), None) \
            or next((item for item in candidates if title and item["title"].strip().casefold()
                     and (title.endswith(item["title"].strip().casefold()) or item["title"].strip().casefold().endswith(title))), None)
        if match:
            candidates.remove(match)
            if match["urls"]:
                window["urls"] = match["urls"]
