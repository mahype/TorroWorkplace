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

from .hyprland import Hyprland, HyprlandError, normal_clients
from .paths import Paths
from .storage import StorageError, Store, atomic_json, now, read_json, validate_profile


class RestoreError(RuntimeError):
    pass


class RestoreCancelled(RuntimeError):
    pass


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


def monitor_changes(saved: list[dict[str, Any]], current: list[dict[str, Any]]) -> list[str]:
    fields = ("width", "height", "x", "y", "scale")
    current_by_name = {item.get("name"): item for item in current}
    changes = []
    for monitor in saved:
        actual = current_by_name.get(monitor.get("name"))
        if actual is None:
            changes.append(f"missing monitor {monitor.get('name')}")
        elif any(actual.get(field) != monitor.get(field) for field in fields):
            changes.append(f"changed monitor {monitor.get('name')}")
    return changes


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
        if str((client.get("workspace") or {}).get("name", "")) == str(expected.get("workspace", "")):
            score += 1
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
        self.available_monitors: set[str] | None = None
        self.workspace_selectors: dict[str, str] = {}

    def restore(self, profile: dict[str, Any], mode: str = "empty", confirm_replace: bool = False) -> dict[str, Any]:
        validate_profile(profile)
        if mode not in {"empty", "add", "replace"}:
            raise RestoreError("invalid restore mode")
        if mode == "replace" and not confirm_replace:
            raise RestoreError("replace mode requires --confirm-replace")
        with RestoreLock(self.paths.lock):
            run_id = uuid.uuid4().hex
            status = {
                "schemaVersion": 1, "operation": "load", "runId": run_id, "pid": os.getpid(),
                "state": "running", "phase": "preparing", "profile": profile["id"], "mode": mode,
                "startedAt": now(), "finishedAt": None, "monitorWarnings": [],
                "total": len(profile["windows"]), "completed": 0, "windows": [],
            }
            self._status(status)
            try:
                self.workspace_selectors = {
                    str(workspace["name"]): self._workspace_selector(workspace)
                    for workspace in profile["workspaces"]
                }
                current_clients = normal_clients(self.hyprland.query("clients"))
                if mode == "empty" and current_clients:
                    raise RestoreError("empty mode requires a session without normal application windows")
                current_monitors = self.hyprland.query("monitors")
                self.available_monitors = {str(monitor.get("name")) for monitor in current_monitors}
                status["monitorWarnings"] = monitor_changes(
                    profile["environment"]["monitors"], current_monitors
                )
                self._status(status)
                try:
                    timeout = float(self.store.config().get("windowTimeoutSeconds", 12))
                except (TypeError, ValueError) as error:
                    raise RestoreError("window timeout must be a number") from error
                if timeout < 0:
                    raise RestoreError("window timeout cannot be negative")
                missing = self._missing_programs(profile)
                if mode == "replace" and missing:
                    raise RestoreError("required programs are unavailable: " + ", ".join(missing))
                if mode == "replace":
                    status["phase"] = "closing"
                    self._status(status)
                    for client in current_clients:
                        self._raise_if_cancelled(run_id)
                        address = str(client.get("address", ""))
                        if address:
                            self.hyprland.close_window(address)
                    self._wait_closed({str(item.get("address")) for item in current_clients}, run_id)

                self._prepare_workspaces(profile, current_monitors, status)
                used = {str(client.get("address")) for client in current_clients} if mode == "add" else set()
                launched_shared: dict[str, subprocess.Popen[Any]] = {}
                restored_addresses: dict[str, str] = {}
                status["phase"] = "launching"
                for expected in self._ordered_windows(profile):
                    self._raise_if_cancelled(run_id)
                    result = {"id": expected["id"], "application": expected.get("application"), "state": "waiting"}
                    status["windows"].append(result)
                    self._status(status)
                    command = expected.get("command", [])
                    if not command:
                        result.update(state="skipped", error="no safe launch command")
                    elif shutil.which(command[0]) is None and not os.path.isabs(command[0]):
                        result.update(state="failed", error=f"program not found: {command[0]}")
                    else:
                        try:
                            key = str(expected.get("application", "")).casefold()
                            shared = "chrom" in key
                            process = launched_shared.get(key) if shared else None
                            if process is None:
                                launch = ["uwsm", "app", "-t", "service", "--", *command] if shutil.which("uwsm") else command
                                process = self.popen(launch, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
                                if shared:
                                    launched_shared[key] = process
                            client = self._wait_match(expected, used, process.pid, timeout, run_id)
                            if client is None:
                                result.update(state="failed", error="window timeout")
                            else:
                                address = str(client["address"])
                                used.add(address)
                                self._place_initial(expected, address)
                                restored_addresses[expected["id"]] = address
                                result.update(state="restored", address=address)
                        except (OSError, HyprlandError) as error:
                            result.update(state="failed", error=str(error))
                    status["completed"] += 1
                    self._status(status)
                self._raise_if_cancelled(run_id)
                status["phase"] = "arranging"
                self._status(status)
                self._rebuild_tiled(profile, restored_addresses, run_id)
                results = {item["id"]: item for item in status["windows"]}
                for expected in profile["windows"]:
                    self._raise_if_cancelled(run_id)
                    address = restored_addresses.get(expected["id"])
                    if not address:
                        continue
                    try:
                        if not expected.get("floating"):
                            geometry = expected.get("geometry") or {}
                            self.hyprland.resize_window(
                                address, int(geometry.get("width", 0)), int(geometry.get("height", 0))
                            )
                        self._apply_final_state(expected, address)
                    except HyprlandError as error:
                        result = results.get(expected["id"])
                        if result:
                            result.update(state="failed", error=str(error))
                        self._status(status)
                active_workspace = self._target_workspace(
                    str(profile.get("environment", {}).get("activeWorkspace") or "")
                )
                if active_workspace:
                    try:
                        self.hyprland.focus_workspace(active_workspace)
                    except HyprlandError as error:
                        status["monitorWarnings"].append(str(error))
                if status["state"] == "running":
                    status["state"] = "completed" if all(item["state"] in {"restored", "skipped"} for item in status["windows"]) else "partial"
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
            if status["state"] in {"completed", "partial"}:
                config = self.store.config()
                config["lastProfile"] = profile["id"]
                self.store.write_config(config)
            return status

    def _ordered_windows(self, profile: dict[str, Any]) -> list[dict[str, Any]]:
        by_id = {window["id"]: window for window in profile["windows"]}
        order: list[str] = []
        for workspace in profile["workspaces"]:
            representative, operations = self._tree_plan(workspace.get("tree") or {})
            if representative:
                order.append(representative)
            order.extend(operation[0] for operation in operations)
        order.extend(identifier for identifier in by_id if identifier not in order)
        return [by_id[identifier] for identifier in order if identifier in by_id]

    def _split_instructions(self, profile: dict[str, Any]) -> dict[str, tuple[str, str, float]]:
        instructions: dict[str, tuple[str, str, float]] = {}
        for workspace in profile["workspaces"]:
            _, operations = self._tree_plan(workspace.get("tree") or {})
            for identifier, parent, direction, ratio in operations:
                instructions[identifier] = (parent, direction, ratio)
        return instructions

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
            event_socket = self.hyprland.event_socket(min(0.25, timeout))
        except (OSError, HyprlandError):
            pass
        try:
            while self.monotonic() < deadline:
                self._raise_if_cancelled(run_id)
                client = match_client(expected, normal_clients(self.hyprland.query("clients")), used, pid)
                if client:
                    return client
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
        return None

    def _wait_closed(self, addresses: set[str], run_id: str) -> None:
        deadline = self.monotonic() + 5
        while self.monotonic() < deadline:
            self._raise_if_cancelled(run_id)
            remaining = {str(item.get("address")) for item in normal_clients(self.hyprland.query("clients"))}
            if not addresses & remaining:
                return
            self.sleep(0.05)
        raise RestoreError("timed out while closing existing windows")

    def _place_initial(self, window: dict[str, Any], address: str) -> None:
        workspace = self._target_workspace(str(window.get("workspace", "")))
        if window.get("floating") and workspace:
            self._move_to_workspace(address, workspace, str(window.get("monitor", "")))
        if window.get("floating"):
            self.hyprland.set_floating(address, True)
            geometry = window.get("geometry") or {}
            self.hyprland.resize_window(address, int(geometry.get("width", 0)), int(geometry.get("height", 0)))
            self.hyprland.move_window(address, x=int(geometry.get("x", 0)), y=int(geometry.get("y", 0)))
        else:
            self.hyprland.set_floating(address, False)

    def _apply_final_state(self, window: dict[str, Any], address: str) -> None:
        if window.get("pinned"):
            self.hyprland.pin_window(address)
        if window.get("fullscreen"):
            self.hyprland.set_fullscreen(address, str(window.get("fullscreenMode", "fullscreen")))

    def _prepare_workspaces(self, profile: dict[str, Any], current_monitors: list[dict[str, Any]],
                            status: dict[str, Any]) -> None:
        available = {str(monitor.get("name")) for monitor in current_monitors}
        for workspace in profile["workspaces"]:
            name, monitor = str(workspace.get("name", "")), str(workspace.get("monitor", ""))
            if name and monitor and monitor not in available:
                warning = f"workspace {name} target monitor is unavailable: {monitor}"
                if warning not in status["monitorWarnings"]:
                    status["monitorWarnings"].append(warning)
        self._status(status)

    def _rebuild_tiled(self, profile: dict[str, Any], addresses: dict[str, str], run_id: str) -> None:
        windows = {window["id"]: window for window in profile["windows"]}
        tiled = [identifier for identifier in self._ordered_window_ids(profile)
                 if identifier in addresses and not windows[identifier].get("floating")]
        if not tiled:
            return
        staging = f"name:__workspace_setups_stage_{run_id[:8]}"
        staged: set[str] = set()
        try:
            for identifier in tiled:
                self._raise_if_cancelled(run_id)
                self.hyprland.move_window(addresses[identifier], workspace=staging)
                staged.add(identifier)
            instructions = self._split_instructions(profile)
            inserted: set[str] = set()
            populated_workspaces: set[str] = set()
            for identifier in tiled:
                self._raise_if_cancelled(run_id)
                window = windows[identifier]
                workspace = self._target_workspace(str(window["workspace"]))
                instruction = instructions.get(identifier)
                if instruction and instruction[0] in inserted:
                    self.hyprland.focus_window(addresses[instruction[0]])
                    self.hyprland.layout(f"preselect {instruction[1]}")
                if workspace not in populated_workspaces:
                    self._move_to_workspace(addresses[identifier], workspace, str(window.get("monitor", "")))
                    populated_workspaces.add(workspace)
                else:
                    self.hyprland.move_window(addresses[identifier], workspace=workspace)
                staged.discard(identifier)
                inserted.add(identifier)
                if instruction and instruction[0] in inserted:
                    self.hyprland.focus_window(addresses[identifier])
                    self.hyprland.layout(f"splitratio {float(instruction[2]) * 2:.4f} exact")
        finally:
            for identifier in staged:
                window = windows[identifier]
                try:
                    self._move_to_workspace(
                        addresses[identifier], self._target_workspace(str(window["workspace"])),
                        str(window.get("monitor", ""))
                    )
                except HyprlandError:
                    pass

    def _move_to_workspace(self, address: str, workspace: str, monitor: str) -> None:
        monitor_available = bool(monitor) and (
            self.available_monitors is None or monitor in self.available_monitors
        )
        if monitor_available:
            self.hyprland.focus_monitor(monitor)
        self.hyprland.move_window(address, workspace=workspace)
        if monitor_available:
            self.hyprland.move_workspace(workspace, monitor)

    def _workspace_selector(self, workspace: dict[str, Any]) -> str:
        name = str(workspace.get("name", ""))
        if int(workspace.get("id") or 0) < 0 and not name.startswith("name:"):
            return f"name:{name}"
        return name

    def _target_workspace(self, name: str) -> str:
        return self.workspace_selectors.get(name, name)

    def _missing_programs(self, profile: dict[str, Any]) -> list[str]:
        missing: set[str] = set()
        for window in profile["windows"]:
            command = window.get("command") or []
            if not command:
                continue
            executable = command[0]
            available = (
                os.path.isfile(executable) and os.access(executable, os.X_OK)
                if os.path.isabs(executable)
                else shutil.which(executable) is not None
            )
            if not available:
                missing.add(executable)
        return sorted(missing)

    def _ordered_window_ids(self, profile: dict[str, Any]) -> list[str]:
        return [window["id"] for window in self._ordered_windows(profile)]

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
