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

from . import SCHEMA_VERSION, SLOTS, STRATEGIES
from .browser import browser_family, restorable_url
from .paths import Paths

TEMPLATE_ID = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
WORKSPACE_NAME = re.compile(r"^[1-9][0-9]*$")
LANGUAGES = ("auto", "en", "de")
UNSAFE_ARGUMENT = re.compile(r"(?i)(token|password|passwd|secret|api[-_]?key|auth)")
SHELLS = {"sh", "bash", "zsh", "fish", "dash"}
INTERPRETER_FLAGS = {
    "python": {"-c", "-m"}, "python3": {"-c", "-m"},
    "perl": {"-e", "-E"}, "ruby": {"-e"}, "node": {"-e", "--eval"},
}
DEFAULT_CONFIG = {
    "schemaVersion": 2,
    "language": "auto",
    "windowTimeoutSeconds": 12,
}


class StorageError(ValueError):
    """Backend error; ``code`` marks errors the UI can resolve with a choice."""

    def __init__(self, message: str, code: str | None = None, **details: Any):
        super().__init__(message)
        self.code = code
        self.details = details


def now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def slugify(name: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", name.strip().lower()).strip("-")
    if not slug or not TEMPLATE_ID.fullmatch(slug):
        raise StorageError("template name must contain letters or numbers")
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


def validate_template(template: Any) -> dict[str, Any]:
    if not isinstance(template, dict) or template.get("schemaVersion") != SCHEMA_VERSION:
        raise StorageError("unsupported template schema")
    required = {"id": str, "name": str, "windows": list, "tree": dict}
    for key, kind in required.items():
        if not isinstance(template.get(key), kind):
            raise StorageError(f"invalid template field: {key}")
    if not TEMPLATE_ID.fullmatch(template["id"]):
        raise StorageError("invalid template id")
    shortcut = template.get("shortcut")
    if shortcut is not None and (isinstance(shortcut, bool) or shortcut not in SLOTS):
        raise StorageError("invalid shortcut slot")
    if not isinstance(template.get("restoreOnStartup", False), bool):
        raise StorageError("invalid startup flag")
    startup_workspace = template.get("startupWorkspace")
    if startup_workspace is not None and not (isinstance(startup_workspace, str) and WORKSPACE_NAME.fullmatch(startup_workspace)):
        raise StorageError("startup workspace must be a workspace number")
    if template.get("restoreOnStartup") and startup_workspace is None:
        raise StorageError("startup restore requires a startup workspace")
    ids: set[str] = set()
    for window in template["windows"]:
        if not isinstance(window, dict) or not isinstance(window.get("id"), str):
            raise StorageError("invalid window")
        if window["id"] in ids:
            raise StorageError("duplicate window id")
        ids.add(window["id"])
    tree_ids: set[str] = set()
    _validate_tree(template["tree"], ids, tree_ids)
    for window in template["windows"]:
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
        if not isinstance(window.get("application"), str):
            raise StorageError(f"invalid application for {window['id']}")
        urls = window.get("urls", [])
        if not isinstance(urls, list) or not all(restorable_url(url) for url in urls):
            raise StorageError(f"invalid urls for {window['id']}")
        if window.get("strategy", "auto") not in STRATEGIES:
            raise StorageError(f"invalid strategy for {window['id']}")
        if any(not isinstance(window.get(field, False), bool) for field in ("floating", "fullscreen", "pinned")):
            raise StorageError(f"invalid state for {window['id']}")
        if window.get("fullscreenMode", "fullscreen") not in {"fullscreen", "maximized"}:
            raise StorageError(f"invalid fullscreen mode for {window['id']}")
        geometry = window.get("geometry")
        if not isinstance(geometry, dict) or any(not _number(geometry.get(field)) for field in ("x", "y", "width", "height")):
            raise StorageError(f"invalid geometry for {window['id']}")
        if geometry["width"] < 0 or geometry["height"] < 0:
            raise StorageError(f"invalid geometry for {window['id']}")
    tiled_ids = {window["id"] for window in template["windows"] if not window.get("floating")}
    if tree_ids != tiled_ids:
        raise StorageError("the workspace tree must contain every tiled window exactly once")
    return template


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

    def template_path(self, identifier: str) -> Path:
        if not TEMPLATE_ID.fullmatch(identifier):
            raise StorageError("invalid template id")
        return self.paths.templates / f"{identifier}.json"

    def get(self, name: str) -> dict[str, Any]:
        identifier = name if TEMPLATE_ID.fullmatch(name) else slugify(name)
        template = read_json(self.template_path(identifier))
        if template is None:
            raise StorageError(f"template not found: {name}", "not_found")
        validated = validate_template(template)
        if validated["id"] != identifier:
            raise StorageError("template id does not match its filename")
        return validated

    def exists(self, name: str) -> bool:
        try:
            return self.template_path(slugify(name)).exists()
        except StorageError:
            return False

    def all(self) -> list[dict[str, Any]]:
        if not self.paths.templates.exists():
            return []
        result = []
        for path in sorted(self.paths.templates.glob("*.json")):
            template = validate_template(read_json(path))
            if template["id"] != path.stem:
                raise StorageError("template id does not match its filename")
            result.append(template)
        return result

    def by_slot(self, slot: int) -> dict[str, Any] | None:
        return next((template for template in self.all() if template.get("shortcut") == slot), None)

    def list(self) -> list[dict[str, Any]]:
        return [summary(template) for template in self.all()]

    def save(self, template: dict[str, Any], replace: bool = False) -> None:
        validate_template(template)
        path = self.template_path(template["id"])
        if path.exists() and not replace:
            raise StorageError("template already exists; use update", "exists", template=template["name"])
        if path.exists():
            self._backup(path)
        atomic_json(path, template)

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
        template = self.get(name)
        self.template_path(template["id"]).unlink()

    def rename(self, old: str, new: str) -> dict[str, Any]:
        template = self.get(old)
        old_id, new_id = template["id"], slugify(new)
        if self.template_path(new_id).exists() and new_id != old_id:
            raise StorageError("target template already exists")
        template.update({"id": new_id, "name": new.strip(), "updatedAt": now()})
        atomic_json(self.template_path(new_id), template)
        if new_id != old_id:
            self.template_path(old_id).unlink()
        return template

    def update_settings(self, name: str, **changes: Any) -> dict[str, Any]:
        template = self.get(name)
        template.update(changes)
        validate_template(template)
        atomic_json(self.template_path(template["id"]), template)
        return template

    def assign_shortcut(self, name: str, slot: int | None, force: bool = False) -> dict[str, Any]:
        template = self.get(name)
        if slot is not None:
            holder = self.by_slot(slot)
            if holder and holder["id"] != template["id"]:
                if not force:
                    raise StorageError(
                        f"F{slot} is already assigned to {holder['name']}", "slot_taken",
                        slot=slot, template=holder["name"],
                    )
                self.update_settings(holder["id"], shortcut=None)
        return self.update_settings(template["id"], shortcut=slot)

    def unique_name(self, base: str) -> str:
        name, counter = base, 2
        while self.exists(name):
            name, counter = f"{base} ({counter})", counter + 1
        return name

    def config(self) -> dict[str, Any]:
        data = read_json(self.paths.config, {})
        if not isinstance(data, dict):
            raise StorageError("invalid config")
        return {**DEFAULT_CONFIG, **data}

    def write_config(self, config: dict[str, Any]) -> None:
        atomic_json(self.paths.config, config)


def summary(template: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": template["id"], "name": template["name"], "updatedAt": template.get("updatedAt"),
        "windowCount": len(template["windows"]),
        "unrestorableCount": sum(not window.get("command") for window in template["windows"]),
        "shortcut": template.get("shortcut"),
        "restoreOnStartup": bool(template.get("restoreOnStartup")),
        "startupWorkspace": template.get("startupWorkspace"),
        "sourceWorkspace": (template.get("source") or {}).get("workspace"),
        "windows": [
            {"id": window["id"], "application": window.get("application", ""),
             "strategy": window.get("strategy", "auto"), "restorable": bool(window.get("command")),
             "urls": list(window.get("urls") or []), "browser": browser_family(window) is not None}
            for window in template["windows"]
        ],
    }
