from __future__ import annotations

import json
import math
import os
import re
import shutil
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from . import SCHEMA_VERSION
from .paths import Paths

PROFILE_ID = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
UNSAFE_ARGUMENT = re.compile(r"(?i)(token|password|passwd|secret|api[-_]?key|auth)")
SHELLS = {"sh", "bash", "zsh", "fish", "dash"}
INTERPRETER_FLAGS = {
    "python": {"-c", "-m"}, "python3": {"-c", "-m"},
    "perl": {"-e", "-E"}, "ruby": {"-e"}, "node": {"-e", "--eval"},
}
DEFAULT_CONFIG = {
    "schemaVersion": 1,
    "startupMode": "none",
    "startupProfile": None,
    "lastProfile": None,
    "windowTimeoutSeconds": 12,
}


class StorageError(ValueError):
    pass


def now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def slugify(name: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", name.strip().lower()).strip("-")
    if not slug or not PROFILE_ID.fullmatch(slug):
        raise StorageError("profile name must contain letters or numbers")
    return slug


def atomic_json(path: Path, value: Any) -> None:
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if path.parent.is_symlink() or path.is_symlink():
        raise StorageError(f"refusing to write symbolic link: {path}")
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(value, handle, ensure_ascii=False, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.chmod(temporary, 0o600)
        os.replace(temporary, path)
        directory_fd = os.open(path.parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


def read_json(path: Path, default: Any = None) -> Any:
    if not path.exists():
        return default
    if path.is_symlink() or not path.is_file():
        raise StorageError(f"unsafe data path: {path}")
    try:
        with path.open(encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, json.JSONDecodeError) as error:
        raise StorageError(f"cannot read {path.name}: {error}") from error


def validate_profile(profile: Any) -> dict[str, Any]:
    if not isinstance(profile, dict) or profile.get("schemaVersion") != SCHEMA_VERSION:
        raise StorageError("unsupported profile schema")
    required = {"id": str, "name": str, "environment": dict, "workspaces": list, "windows": list}
    for key, kind in required.items():
        if not isinstance(profile.get(key), kind):
            raise StorageError(f"invalid profile field: {key}")
    if not PROFILE_ID.fullmatch(profile["id"]):
        raise StorageError("invalid profile id")
    monitors = profile["environment"].get("monitors")
    if not isinstance(monitors, list):
        raise StorageError("invalid monitor list")
    for monitor in monitors:
        if not isinstance(monitor, dict) or not isinstance(monitor.get("name"), str):
            raise StorageError("invalid monitor")
        if any(not _number(monitor.get(field)) for field in ("width", "height", "x", "y", "scale")):
            raise StorageError("invalid monitor geometry")
    ids: set[str] = set()
    for window in profile["windows"]:
        if not isinstance(window, dict) or not isinstance(window.get("id"), str):
            raise StorageError("invalid window")
        if window["id"] in ids:
            raise StorageError("duplicate window id")
        ids.add(window["id"])
    tree_ids: set[str] = set()
    for workspace in profile["workspaces"]:
        if not isinstance(workspace, dict) or not isinstance(workspace.get("name"), str) or not isinstance(workspace.get("tree", {}), dict):
            raise StorageError("invalid workspace")
        _validate_tree(workspace.get("tree", {}), ids, tree_ids)
    for window in profile["windows"]:
        command = window.get("command")
        if not isinstance(command, list) or not all(isinstance(arg, str) and "\0" not in arg for arg in command):
            raise StorageError(f"invalid command for {window['id']}")
        if not command and window.get("restorable", True):
            raise StorageError(f"missing command for {window['id']}")
        if command:
            executable = command[0]
            if ("/" in executable and not executable.startswith("/")) or executable in {"sh", "bash", "zsh", "fish", "env"}:
                raise StorageError(f"unsafe executable for {window['id']}")
            if any(arg.startswith("/tmp/") or UNSAFE_ARGUMENT.search(arg.split("=", 1)[0]) for arg in command[1:]):
                raise StorageError(f"unsafe argument for {window['id']}")
            if _has_embedded_interpreter(command):
                raise StorageError(f"embedded shell command for {window['id']}")
        for field in ("application", "workspace", "monitor"):
            if not isinstance(window.get(field), str):
                raise StorageError(f"invalid {field} for {window['id']}")
        if any(not isinstance(window.get(field, False), bool) for field in ("floating", "fullscreen", "pinned")):
            raise StorageError(f"invalid state for {window['id']}")
        if window.get("fullscreenMode", "fullscreen") not in {"fullscreen", "maximized"}:
            raise StorageError(f"invalid fullscreen mode for {window['id']}")
        geometry = window.get("geometry")
        if not isinstance(geometry, dict) or any(not _number(geometry.get(field)) for field in ("x", "y", "width", "height")):
            raise StorageError(f"invalid geometry for {window['id']}")
        if geometry["width"] < 0 or geometry["height"] < 0:
            raise StorageError(f"invalid geometry for {window['id']}")
    tiled_ids = {window["id"] for window in profile["windows"] if not window.get("floating")}
    if tree_ids != tiled_ids:
        raise StorageError("workspace trees must contain every tiled window exactly once")
    return profile


def _number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def _has_embedded_interpreter(command: list[str]) -> bool:
    for index, argument in enumerate(command):
        executable = Path(argument).name
        trailing = set(command[index + 1:])
        if executable in SHELLS and any(flag.startswith("-") and "c" in flag[1:] for flag in trailing):
            return True
        family = executable.rstrip("0123456789.")
        if family in INTERPRETER_FLAGS and trailing & INTERPRETER_FLAGS[family]:
            return True
    return False


def _validate_tree(node: dict[str, Any], window_ids: set[Any], seen: set[str]) -> None:
    if not node:
        return
    if "window" in node:
        identifier = node.get("window")
        if not isinstance(identifier, str) or identifier not in window_ids or identifier in seen:
            raise StorageError("invalid or duplicate window in workspace tree")
        if set(node) != {"window"}:
            raise StorageError("invalid workspace tree leaf")
        seen.add(identifier)
        return
    if "fallback" in node:
        fallback = node.get("fallback")
        if set(node) != {"fallback"} or not isinstance(fallback, list) or not fallback:
            raise StorageError("invalid workspace tree fallback")
        for identifier in fallback:
            if not isinstance(identifier, str) or identifier not in window_ids or identifier in seen:
                raise StorageError("invalid or duplicate fallback window")
            seen.add(identifier)
        return
    if set(node) != {"split", "ratio", "first", "second"}:
        raise StorageError("invalid workspace tree node")
    if node.get("split") not in {"horizontal", "vertical"}:
        raise StorageError("invalid workspace tree split")
    ratio = node.get("ratio")
    if not _number(ratio) or not 0.05 <= float(ratio) <= 0.95:
        raise StorageError("invalid workspace tree ratio")
    if not isinstance(node.get("first"), dict) or not isinstance(node.get("second"), dict):
        raise StorageError("invalid workspace tree children")
    _validate_tree(node["first"], window_ids, seen)
    _validate_tree(node["second"], window_ids, seen)


class Store:
    def __init__(self, paths: Paths | None = None, backup_limit: int = 5):
        self.paths = paths or Paths.from_env()
        self.backup_limit = backup_limit

    def profile_path(self, identifier: str) -> Path:
        if not PROFILE_ID.fullmatch(identifier):
            raise StorageError("invalid profile id")
        return self.paths.profiles / f"{identifier}.json"

    def get(self, name: str) -> dict[str, Any]:
        identifier = name if PROFILE_ID.fullmatch(name) else slugify(name)
        profile = read_json(self.profile_path(identifier))
        if profile is None:
            raise StorageError(f"profile not found: {name}")
        validated = validate_profile(profile)
        if validated["id"] != identifier:
            raise StorageError("profile id does not match its filename")
        return validated

    def list(self) -> list[dict[str, Any]]:
        config = self.config()
        if not self.paths.profiles.exists():
            return []
        result = []
        for path in sorted(self.paths.profiles.glob("*.json")):
            profile = validate_profile(read_json(path))
            if profile["id"] != path.stem:
                raise StorageError("profile id does not match its filename")
            result.append({
                "id": profile["id"], "name": profile["name"],
                "updatedAt": profile.get("updatedAt"),
                "workspaceCount": len(profile["workspaces"]), "windowCount": len(profile["windows"]),
                "startup": config.get("startupProfile") == profile["id"],
                "lastUsed": config.get("lastProfile") == profile["id"],
                "unrestorableCount": sum(not window.get("command") for window in profile["windows"]),
            })
        return result

    def save(self, profile: dict[str, Any], replace: bool = False) -> None:
        validate_profile(profile)
        path = self.profile_path(profile["id"])
        if path.exists() and not replace:
            raise StorageError("profile already exists; use update")
        if path.exists():
            self._backup(path)
        atomic_json(path, profile)

    def _backup(self, path: Path) -> None:
        folder = self.paths.backups / path.stem
        folder.mkdir(mode=0o700, parents=True, exist_ok=True)
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
        target = folder / f"{stamp}.json"
        if path.is_symlink():
            raise StorageError("refusing to back up symbolic link")
        shutil.copyfile(path, target)
        os.chmod(target, 0o600)
        for stale in sorted(folder.glob("*.json"), reverse=True)[self.backup_limit:]:
            stale.unlink()

    def delete(self, name: str) -> None:
        profile = self.get(name)
        self.profile_path(profile["id"]).unlink()
        config = self.config()
        for key in ("startupProfile", "lastProfile"):
            if config.get(key) == profile["id"]:
                config[key] = None
        self.write_config(config)

    def rename(self, old: str, new: str) -> dict[str, Any]:
        profile = self.get(old)
        old_id, new_id = profile["id"], slugify(new)
        if self.profile_path(new_id).exists() and new_id != old_id:
            raise StorageError("target profile already exists")
        profile.update({"id": new_id, "name": new.strip(), "updatedAt": now()})
        atomic_json(self.profile_path(new_id), profile)
        if new_id != old_id:
            self.profile_path(old_id).unlink()
        config = self.config()
        for key in ("startupProfile", "lastProfile"):
            if config.get(key) == old_id:
                config[key] = new_id
        self.write_config(config)
        return profile

    def config(self) -> dict[str, Any]:
        data = read_json(self.paths.config, {})
        if not isinstance(data, dict):
            raise StorageError("invalid config")
        return {**DEFAULT_CONFIG, **data}

    def write_config(self, config: dict[str, Any]) -> None:
        atomic_json(self.paths.config, config)
