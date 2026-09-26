from __future__ import annotations

import fcntl
import os
import shutil
import subprocess
import time
import uuid
from contextlib import AbstractContextManager
from pathlib import Path
from typing import Any, Callable

from .browser import browser_family
from .hyprland import Hyprland, HyprlandError, normal_clients
from .paths import Paths
from .storage import StorageError, Store, atomic_json, now, read_json, validate_template


class RestoreError(StorageError):
    pass


class RestoreCancelled(RuntimeError):
    pass


MODES = ("ask", "add", "replace")
# How long a single-instance candidate may take to show a fresh window before
# the already running one is moved over instead.
AUTO_FALLBACK_SECONDS = 3.0


class RestoreLock(AbstractContextManager["RestoreLock"]):
    def __init__(self, path: Path):
        self.path = path
        self.handle: Any = None

    def __enter__(self) -> "RestoreLock":
        self.path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.handle = self.path.open("a+", encoding="ascii")
        try:
            fcntl.flock(self.handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            self.handle.close()
            raise RestoreError("another restore is already running") from error
        self.handle.seek(0)
        self.handle.truncate()
        self.handle.write(str(os.getpid()))
        self.handle.flush()
        return self

    def __exit__(self, *args: Any) -> None:
        if self.handle:
            fcntl.flock(self.handle, fcntl.LOCK_UN)
            self.handle.close()


def process_ancestors(pid: int, proc: Path = Path("/proc")) -> set[int]:
    result = set()
    while pid > 1 and pid not in result:
        result.add(pid)
        try:
            lines = (proc / str(pid) / "status").read_text().splitlines()
            pid = int(next(line.split()[1] for line in lines if line.startswith("PPid:")))
        except (OSError, StopIteration, ValueError):
            break
    return result


def match_client(expected: dict[str, Any], clients: list[dict[str, Any]], used: set[str], launch_pid: int | None = None) -> dict[str, Any] | None:
    best: tuple[int, dict[str, Any]] | None = None
    expected_classes = {str(expected.get(key, "")).casefold() for key in ("initialClass", "class", "application")} - {""}
    title = str(expected.get("titleHint", "")).casefold()
    for client in clients:
        address = str(client.get("address", ""))
        if not address or address in used:
            continue
        classes = {str(client.get(key, "")).casefold() for key in ("initialClass", "class")} - {""}
        if not expected_classes & classes:
            continue
        score = 0
        score += 8
        if title and title in str(client.get("title", "")).casefold():
            score += 3
        if launch_pid and launch_pid in process_ancestors(int(client.get("pid") or 0)):
            score += 10
        if score and (best is None or score > best[0]):
            best = (score, client)
    return best[1] if best else None


class RestoreCoordinator:
    def __init__(
        self, store: Store | None = None, hyprland: Hyprland | None = None,
        popen: Callable[..., subprocess.Popen[Any]] = subprocess.Popen,
        monotonic: Callable[[], float] = time.monotonic, sleep: Callable[[float], None] = time.sleep,
    ):
        self.store = store or Store()
        self.paths: Paths = self.store.paths
        self.hyprland = hyprland or Hyprland()
        self.popen = popen
        self.monotonic = monotonic
        self.sleep = sleep

    def restore(self, template: dict[str, Any], mode: str = "ask", workspace: str | None = None,
                focus: bool = True) -> dict[str, Any]:
        """Rebuild a template on ``workspace`` (default: the focused one).

        ``ask`` refuses a workspace that already has windows so the caller can
        offer ``add`` (keep them) or ``replace`` (close them first).
        """
        validate_template(template)
        if mode not in MODES:
            raise RestoreError("invalid restore mode")
        target = self._resolve_target(workspace)
        occupied = [client for client in normal_clients(self.hyprland.query("clients"))
                    if self._on_target(client, target)]
        if occupied and mode == "ask":
            raise RestoreError(
                f"workspace {target['name']} is not empty", "workspace_not_empty",
                template=template["name"], workspace=target["name"], windows=len(occupied),
            )
        with RestoreLock(self.paths.lock):
            run_id = uuid.uuid4().hex
            status = {
                "schemaVersion": 2, "operation": "load", "runId": run_id, "pid": os.getpid(),
                "state": "running", "phase": "preparing", "template": template["name"], "mode": mode,
                "workspace": target["name"], "startedAt": now(), "finishedAt": None, "warnings": [],
                "total": len(template["windows"]), "completed": 0, "windows": [],
            }
            self._status(status)
            try:
                try:
                    timeout = float(self.store.config().get("windowTimeoutSeconds", 12))
                except (TypeError, ValueError) as error:
                    raise RestoreError("window timeout must be a number") from error
                if timeout < 0:
                    raise RestoreError("window timeout cannot be negative")
                if mode == "replace" and occupied:
                    status["phase"] = "closing"
                    self._status(status)
                    for client in occupied:
                        self._raise_if_cancelled(run_id)
                        self.hyprland.close_window(str(client.get("address")))
                    self._wait_closed({str(client.get("address")) for client in occupied}, run_id)

                existing = {str(client.get("address")) for client in normal_clients(self.hyprland.query("clients"))}
                used: set[str] = set(existing)
                assigned: set[str] = set()
                restored_addresses: dict[str, str] = {}
                status["phase"] = "launching"
                for expected in self._ordered_windows(template):
                    self._raise_if_cancelled(run_id)
                    result = {"id": expected["id"], "application": expected.get("application"), "state": "waiting"}
                    status["windows"].append(result)
                    self._status(status)
                    try:
                        client, how = self._obtain(expected, used, assigned, timeout, run_id)
                        if client is None:
                            result.update(state="failed" if how else "skipped", error=how or "no safe launch command")
                        else:
                            address = str(client["address"])
                            used.add(address)
                            assigned.add(address)
                            self._place_initial(expected, address, target)
                            restored_addresses[expected["id"]] = address
                            result.update(state="moved" if how == "moved" else "restored", address=address)
                    except (OSError, HyprlandError) as error:
                        result.update(state="failed", error=str(error))
                    status["completed"] += 1
                    self._status(status)
                self._raise_if_cancelled(run_id)
                status["phase"] = "arranging"
                self._status(status)
                self._rebuild_tiled(template, restored_addresses, target, run_id)
                results = {item["id"]: item for item in status["windows"]}
                same_monitor = self._same_monitor_size(template, target)
                for expected in template["windows"]:
                    self._raise_if_cancelled(run_id)
                    address = restored_addresses.get(expected["id"])
                    if not address:
                        continue
                    try:
                        if not expected.get("floating") and same_monitor:
                            geometry = expected.get("geometry") or {}
                            self.hyprland.resize_window(
                                address, int(geometry.get("width", 0)), int(geometry.get("height", 0))
                            )
                        self._apply_final_state(expected, address)
                    except HyprlandError as error:
                        results[expected["id"]].update(state="failed", error=str(error))
                        self._status(status)
                if focus:
                    try:
                        self.hyprland.focus_workspace(target["selector"])
                    except HyprlandError as error:
                        status["warnings"].append(str(error))
                status["state"] = "completed" if all(
                    item["state"] in {"restored", "moved", "skipped"} for item in status["windows"]
                ) else "partial"
                status["phase"] = status["state"]
            except RestoreCancelled:
                status["state"] = "cancelled"
                status["phase"] = "cancelled"
            except Exception:
                status["state"] = "failed"
                status["phase"] = "failed"
                raise
            finally:
                status["finishedAt"] = now()
                self._status(status)
                self._clear_cancel_marker(run_id)
            return status

    def _resolve_target(self, workspace: str | None) -> dict[str, Any]:
        if workspace is None:
            active = self.hyprland.query("activeworkspace")
            name = str(active.get("name", active.get("id")))
            identifier = active.get("id")
        else:
            name = str(workspace).strip()
            if not name:
                raise RestoreError("invalid target workspace")
            known = next((item for item in self.hyprland.query("workspaces") if str(item.get("name")) == name), None)
            identifier = known.get("id") if known else (int(name) if name.isdigit() else None)
        if identifier is not None and int(identifier) <= 0 and not name.isdigit():
            selector = f"name:{name}"
        else:
            selector = name
        if name.startswith("special"):
            raise RestoreError("templates cannot be restored on special workspaces")
        return {"id": identifier, "name": name, "selector": selector}

    @staticmethod
    def _on_target(client: dict[str, Any], target: dict[str, Any]) -> bool:
        workspace = client.get("workspace") or {}
        if target["id"] is not None:
            return workspace.get("id") == target["id"]
        return str(workspace.get("name", "")) == target["name"]

    def _obtain(self, expected: dict[str, Any], used: set[str], assigned: set[str], timeout: float,
                run_id: str) -> tuple[dict[str, Any] | None, str | None]:
        """Return a window for ``expected`` and how it was obtained.

        ``new`` always launches; ``move`` takes an already open window from any
        workspace and only launches when there is none; ``auto`` launches and
        falls back to moving when no fresh window appears in time.
        """
        strategy = expected.get("strategy", "auto")
        movable = self._movable(expected, assigned) if strategy in {"move", "auto"} else None
        if strategy == "move" and movable:
            return movable, "moved"
        command = expected.get("command") or []
        if not command:
            return (movable, "moved") if movable else (None, None)
        if shutil.which(command[0]) is None and not os.path.isabs(command[0]):
            return (movable, "moved") if movable else (None, f"program not found: {command[0]}")
        urls = expected.get("urls") or []
        if urls and browser_family(expected):
            # A running browser would add bare URLs as tabs to its last window.
            command = [*command, "--new-window", *urls]
        launch = ["uwsm", "app", "-t", "service", "--", *command] if shutil.which("uwsm") else command
        process = self.popen(launch, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL, start_new_session=True)
        wait = min(timeout, AUTO_FALLBACK_SECONDS) if movable else timeout
        client = self._wait_match(expected, used, process.pid, wait, run_id)
        if client:
            return client, "launched"
        if movable:
            # A single-instance application may raise its window lazily; look
            # again so the freshest state (and title) is used.
            return self._movable(expected, assigned) or movable, "moved"
        return None, "window timeout"

    def _movable(self, expected: dict[str, Any], assigned: set[str]) -> dict[str, Any] | None:
        return match_client(expected, normal_clients(self.hyprland.query("clients")), assigned)

    def _ordered_windows(self, template: dict[str, Any]) -> list[dict[str, Any]]:
        by_id = {window["id"]: window for window in template["windows"]}
        representative, operations = self._tree_plan(template.get("tree") or {})
        order: list[str] = [representative] if representative else []
        order.extend(operation[0] for operation in operations)
        order.extend(identifier for identifier in by_id if identifier not in order)
        return [by_id[identifier] for identifier in order if identifier in by_id]

    def _tree_plan(self, node: dict[str, Any]) -> tuple[str | None, list[tuple[str, str, str, float]]]:
        if isinstance(node.get("window"), str):
            return node["window"], []
        fallback = node.get("fallback")
        if isinstance(fallback, list) and fallback:
            identifiers = [identifier for identifier in fallback if isinstance(identifier, str)]
            operations = [
                (identifier, identifiers[index - 1], "r", 0.5)
                for index, identifier in enumerate(identifiers[1:], 1)
            ]
            return identifiers[0], operations
        first, second = node.get("first"), node.get("second")
        if not isinstance(first, dict) or not isinstance(second, dict):
            return None, []
        first_rep, first_operations = self._tree_plan(first)
        second_rep, second_operations = self._tree_plan(second)
        if not first_rep or not second_rep:
            return first_rep or second_rep, first_operations + second_operations
        direction = "r" if node.get("split") == "vertical" else "d"
        ratio = max(0.05, min(0.95, float(node.get("ratio", 0.5))))
        root_operation = (second_rep, first_rep, direction, ratio)
        return first_rep, [root_operation, *first_operations, *second_operations]

    def _wait_match(self, expected: dict[str, Any], used: set[str], pid: int, timeout: float,
                    run_id: str) -> dict[str, Any] | None:
        deadline = self.monotonic() + timeout
        event_socket = None
        try:
            event_socket = self.hyprland.event_socket(min(0.25, timeout) or 0.05)
        except (OSError, HyprlandError):
            pass
        try:
            while True:
                self._raise_if_cancelled(run_id)
                client = match_client(expected, normal_clients(self.hyprland.query("clients")), used, pid)
                if client:
                    return client
                if self.monotonic() >= deadline:
                    return None
                if event_socket:
                    try:
                        event_socket.recv(4096)
                    except (OSError, TimeoutError):
                        pass
                else:
                    self.sleep(min(0.1, timeout))
        finally:
            if event_socket:
                event_socket.close()

    def _wait_closed(self, addresses: set[str], run_id: str) -> None:
        deadline = self.monotonic() + 5
        while self.monotonic() < deadline:
            self._raise_if_cancelled(run_id)
            remaining = {str(item.get("address")) for item in normal_clients(self.hyprland.query("clients"))}
            if not addresses & remaining:
                return
            self.sleep(0.05)
        raise RestoreError("timed out while closing existing windows")

    def _target_origin(self, target: dict[str, Any]) -> tuple[int, int]:
        monitors = self.hyprland.query("monitors")
        workspace = next((item for item in self.hyprland.query("workspaces")
                          if str(item.get("name")) == target["name"]), None)
        name = workspace.get("monitor") if workspace else None
        monitor = next((item for item in monitors if item.get("name") == name), None) \
            or next((item for item in monitors if item.get("focused")), None) \
            or (monitors[0] if monitors else {})
        return int(monitor.get("x", 0)), int(monitor.get("y", 0))

    def _same_monitor_size(self, template: dict[str, Any], target: dict[str, Any]) -> bool:
        saved = (template.get("source") or {}).get("monitorSize") or {}
        workspace = next((item for item in self.hyprland.query("workspaces")
                          if str(item.get("name")) == target["name"]), None)
        monitor = next((item for item in self.hyprland.query("monitors")
                        if workspace and item.get("name") == workspace.get("monitor")), None)
        return bool(monitor) and all(monitor.get(key) == saved.get(key) for key in ("width", "height", "scale"))

    def _place_initial(self, window: dict[str, Any], address: str, target: dict[str, Any]) -> None:
        if window.get("floating"):
            self.hyprland.move_window(address, workspace=target["selector"])
            self.hyprland.set_floating(address, True)
            geometry = window.get("geometry") or {}
            origin_x, origin_y = self._target_origin(target)
            self.hyprland.resize_window(address, int(geometry.get("width", 0)), int(geometry.get("height", 0)))
            self.hyprland.move_window(address, x=origin_x + int(geometry.get("x", 0)),
                                      y=origin_y + int(geometry.get("y", 0)))
        else:
            self.hyprland.set_floating(address, False)

    def _apply_final_state(self, window: dict[str, Any], address: str) -> None:
        if window.get("pinned"):
            self.hyprland.pin_window(address)
        if window.get("fullscreen"):
            self.hyprland.set_fullscreen(address, str(window.get("fullscreenMode", "fullscreen")))

    def _rebuild_tiled(self, template: dict[str, Any], addresses: dict[str, str], target: dict[str, Any],
                       run_id: str) -> None:
        windows = {window["id"]: window for window in template["windows"]}
        tiled = [window["id"] for window in self._ordered_windows(template)
                 if window["id"] in addresses and not window.get("floating")]
        if not tiled:
            return
        staging = f"name:__workspace_setups_stage_{run_id[:8]}"
        staged: set[str] = set()
        try:
            for identifier in tiled:
                self._raise_if_cancelled(run_id)
                self.hyprland.move_window(addresses[identifier], workspace=staging)
                staged.add(identifier)
            _, operations = self._tree_plan(template.get("tree") or {})
            instructions = {identifier: (parent, direction, ratio) for identifier, parent, direction, ratio in operations}
            inserted: set[str] = set()
            for identifier in tiled:
                self._raise_if_cancelled(run_id)
                instruction = instructions.get(identifier)
                if instruction and instruction[0] in inserted:
                    self.hyprland.focus_window(addresses[instruction[0]])
                    self.hyprland.layout(f"preselect {instruction[1]}")
                self.hyprland.move_window(addresses[identifier], workspace=target["selector"])
                staged.discard(identifier)
                inserted.add(identifier)
                if instruction and instruction[0] in inserted:
                    self.hyprland.focus_window(addresses[identifier])
                    self.hyprland.layout(f"splitratio {float(instruction[2]) * 2:.4f} exact")
        finally:
            for identifier in staged:
                try:
                    self.hyprland.move_window(addresses[identifier], workspace=target["selector"])
                except HyprlandError:
                    pass

    def _raise_if_cancelled(self, run_id: str) -> None:
        try:
            marker = read_json(self.paths.cancel, {})
        except StorageError:
            return
        if isinstance(marker, dict) and marker.get("runId") == run_id:
            raise RestoreCancelled()

    def _clear_cancel_marker(self, run_id: str) -> None:
        try:
            marker = read_json(self.paths.cancel, {})
        except StorageError:
            self.paths.cancel.unlink(missing_ok=True)
            return
        if not isinstance(marker, dict) or marker.get("runId") == run_id:
            self.paths.cancel.unlink(missing_ok=True)

    def _status(self, status: dict[str, Any]) -> None:
        atomic_json(self.paths.status, status)
