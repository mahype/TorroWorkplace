import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from workspace_setups.hyprland import HyprlandError
from workspace_setups.restore import RestoreCoordinator, RestoreError, match_client, monitor_changes
from workspace_setups.storage import atomic_json

from .helpers import FakeHyprland, profile, store


def client(address: str = "0x1", title: str = "Terminal") -> dict:
    return {"address": address, "class": "foot", "initialClass": "foot", "title": title, "pid": 100,
            "mapped": True, "hidden": False, "workspace": {"id": 1, "name": "1"}}


class Process:
    pid = 99


class RestoreTests(unittest.TestCase):
    def test_monitor_drift(self) -> None:
        saved = profile()["environment"]["monitors"]
        self.assertEqual(monitor_changes(saved, saved), [])
        changed = [dict(saved[0], width=100)]
        self.assertEqual(monitor_changes(saved, changed), ["changed monitor DP-1"])
        self.assertEqual(monitor_changes(saved, []), ["missing monitor DP-1"])

    def test_match_uses_class_title_and_used_addresses(self) -> None:
        expected = profile()["windows"][0]
        clients = [client("0x1", "Other"), client("0x2", "Terminal project")]
        with patch("workspace_setups.restore.process_ancestors", return_value=set()):
            self.assertEqual(match_client(expected, clients, set())["address"], "0x2")
            self.assertEqual(match_client(expected, clients, {"0x2"})["address"], "0x1")

    def test_match_rejects_wrong_class_even_for_launch_descendant(self) -> None:
        expected = profile()["windows"][0]
        helper = dict(client(), **{"class": "splash", "initialClass": "splash"})
        with patch("workspace_setups.restore.process_ancestors", return_value={99}):
            self.assertIsNone(match_client(expected, [helper], set(), 99))

    def test_empty_mode_never_closes_existing_windows(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            hypr = FakeHyprland([client()])
            with self.assertRaisesRegex(RestoreError, "empty mode"):
                RestoreCoordinator(store(Path(folder)), hypr).restore(profile())
            self.assertEqual(hypr.dispatched, [])

    def test_replace_requires_confirmation(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            with self.assertRaisesRegex(RestoreError, "confirm-replace"):
                RestoreCoordinator(store(Path(folder)), FakeHyprland()).restore(profile(), "replace")

    def test_replace_preflight_fails_before_closing_windows(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            normal = client()
            hypr = FakeHyprland([normal])
            backend = store(Path(folder))
            backend.write_config(dict(backend.config(), windowTimeoutSeconds="invalid"))
            with self.assertRaisesRegex(RestoreError, "timeout"):
                RestoreCoordinator(backend, hypr).restore(profile(), "replace", True)
            self.assertNotIn(("close_window", "0x1"), hypr.dispatched)

            backend.write_config(dict(backend.config(), windowTimeoutSeconds=1))
            value = profile()
            value["windows"][0]["command"] = ["definitely-missing-workspace-setups-program"]
            with patch("workspace_setups.restore.shutil.which", return_value=None):
                with self.assertRaisesRegex(RestoreError, "unavailable"):
                    RestoreCoordinator(backend, hypr).restore(value, "replace", True)
            self.assertNotIn(("close_window", "0x1"), hypr.dispatched)

    def test_replace_closes_only_normal_clients_and_targets_address(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            normal = client()
            special = dict(client("0x2"), workspace={"id": -1, "name": "special"})
            hypr = FakeHyprland([normal, special])
            backend = store(Path(folder))
            backend.write_config(dict(backend.config(), windowTimeoutSeconds=0))
            with patch("workspace_setups.restore.shutil.which", return_value="/usr/bin/foot"):
                result = RestoreCoordinator(backend, hypr, popen=lambda *a, **k: Process()).restore(profile(), "replace", True)
            self.assertEqual(hypr.dispatched[0], ("close_window", "0x1"))
            self.assertEqual(result["state"], "partial")

    def test_successful_restore_places_by_address(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            hypr = FakeHyprland()
            backend = store(Path(folder))

            def launch(*args, **kwargs):
                hypr.clients.append(client())
                return Process()

            with patch("workspace_setups.restore.shutil.which", return_value="/usr/bin/foot"), patch("workspace_setups.restore.process_ancestors", return_value={99}):
                result = RestoreCoordinator(backend, hypr, popen=launch).restore(profile(), "add")
            self.assertEqual(result["state"], "completed")
            self.assertIn(("set_floating", "0x1:False"), hypr.dispatched)

    def test_tree_drives_dwindle_preselection_and_ratio(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            hypr = FakeHyprland()
            backend = store(Path(folder))
            value = profile()
            second = dict(value["windows"][0], id="window-2", titleHint="Second")
            value["windows"].append(second)
            value["workspaces"][0]["tree"] = {
                "split": "vertical", "ratio": 0.3,
                "first": {"window": "window-1"}, "second": {"window": "window-2"},
            }
            launched = 0

            def launch(*args, **kwargs):
                nonlocal launched
                launched += 1
                hypr.clients.append(client(f"0x{launched}", "Terminal" if launched == 1 else "Second"))
                return Process()

            with patch("workspace_setups.restore.shutil.which", return_value="/usr/bin/foot"), patch("workspace_setups.restore.process_ancestors", return_value={99}):
                result = RestoreCoordinator(backend, hypr, popen=launch).restore(value)
            self.assertEqual(result["state"], "completed")
            self.assertIn(("focus_window", "0x1"), hypr.dispatched)
            self.assertIn(("layout", "preselect r"), hypr.dispatched)
            self.assertIn(("layout", "splitratio 0.6000 exact"), hypr.dispatched)

    def test_balanced_tree_uses_parent_first_insertion_plan(self) -> None:
        value = profile()
        value["windows"] = [
            dict(value["windows"][0], id=identifier)
            for identifier in ("a", "b", "c", "d")
        ]
        value["workspaces"][0]["tree"] = {
            "split": "vertical", "ratio": 0.5,
            "first": {
                "split": "horizontal", "ratio": 0.4,
                "first": {"window": "a"}, "second": {"window": "b"},
            },
            "second": {
                "split": "horizontal", "ratio": 0.6,
                "first": {"window": "c"}, "second": {"window": "d"},
            },
        }
        coordinator = RestoreCoordinator(store(Path("/tmp/not-used")), FakeHyprland())
        self.assertEqual(coordinator._ordered_window_ids(value), ["a", "c", "b", "d"])
        self.assertEqual(coordinator._split_instructions(value), {
            "c": ("a", "r", 0.5),
            "b": ("a", "d", 0.4),
            "d": ("c", "d", 0.6),
        })

    def test_cancel_marker_stops_before_launch(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            backend = store(Path(folder))
            coordinator = RestoreCoordinator(backend, FakeHyprland())
            # A cancellation from before this restore is intentionally cleared.
            backend.paths.state_dir.mkdir(parents=True)
            backend.paths.cancel.write_text("old")
            value = profile()
            value["windows"] = []
            value["workspaces"][0]["tree"] = {}
            result = coordinator.restore(value)
            self.assertEqual(result["state"], "completed")
            self.assertFalse(backend.paths.cancel.exists())

    def test_current_run_cancellation_stops_before_launch(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            backend = store(Path(folder))

            class CancellingCoordinator(RestoreCoordinator):
                cancelled = False

                def _status(self, status):
                    super()._status(status)
                    if status.get("state") == "running" and not self.cancelled:
                        self.cancelled = True
                        atomic_json(self.paths.cancel, {"runId": status["runId"]})

            coordinator = CancellingCoordinator(
                backend, FakeHyprland(), popen=lambda *args, **kwargs: self.fail("application was launched")
            )
            result = coordinator.restore(profile())
            self.assertEqual(result["state"], "cancelled")

    def test_workspaces_are_created_on_saved_monitor_and_active_workspace_restored(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            hypr = FakeHyprland()
            value = profile()
            value["environment"]["activeWorkspace"] = "1"

            def launch(*args, **kwargs):
                hypr.clients.append(client())
                return Process()

            with patch("workspace_setups.restore.shutil.which", return_value="/usr/bin/foot"), patch(
                "workspace_setups.restore.process_ancestors", return_value={99}
            ):
                result = RestoreCoordinator(store(Path(folder)), hypr, popen=launch).restore(value)
            self.assertEqual(result["state"], "completed")
            self.assertIn(("focus_monitor", "DP-1"), hypr.dispatched)
            self.assertIn(("move_workspace", "1:DP-1"), hypr.dispatched)
            self.assertEqual(hypr.dispatched[-1], ("focus_workspace", "1"))

    def test_missing_monitor_falls_back_without_leaving_staged_window(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            hypr = FakeHyprland()
            value = profile()
            value["environment"]["monitors"][0]["name"] = "HDMI-A-1"
            value["workspaces"][0]["monitor"] = "HDMI-A-1"
            value["windows"][0]["monitor"] = "HDMI-A-1"

            def launch(*args, **kwargs):
                hypr.clients.append(client())
                return Process()

            with patch("workspace_setups.restore.shutil.which", return_value="/usr/bin/foot"), patch(
                "workspace_setups.restore.process_ancestors", return_value={99}
            ):
                result = RestoreCoordinator(store(Path(folder)), hypr, popen=launch).restore(value)
            self.assertEqual(result["state"], "completed")
            self.assertTrue(result["monitorWarnings"])
            self.assertIn(("move_window", "0x1:1"), hypr.dispatched)
            self.assertNotIn(("focus_monitor", "HDMI-A-1"), hypr.dispatched)

    def test_named_workspace_uses_hyprland_name_selector(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            hypr = FakeHyprland()
            value = profile()
            value["workspaces"][0].update(id=-1337, name="project")
            value["windows"][0]["workspace"] = "project"
            value["environment"]["activeWorkspace"] = "project"

            def launch(*args, **kwargs):
                mapped = client()
                mapped["workspace"] = {"id": -1337, "name": "project"}
                hypr.clients.append(mapped)
                return Process()

            with patch("workspace_setups.restore.shutil.which", return_value="/usr/bin/foot"), patch(
                "workspace_setups.restore.process_ancestors", return_value={99}
            ):
                result = RestoreCoordinator(store(Path(folder)), hypr, popen=launch).restore(value)
            self.assertEqual(result["state"], "completed")
            self.assertIn(("move_window", "0x1:name:project"), hypr.dispatched)
            self.assertIn(("move_workspace", "name:project:DP-1"), hypr.dispatched)
            self.assertEqual(hypr.dispatched[-1], ("focus_workspace", "name:project"))

    def test_rebuild_failure_moves_staged_windows_back(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            value = profile()
            second = dict(value["windows"][0], id="window-2")
            value["windows"].append(second)
            value["workspaces"][0]["tree"] = {
                "split": "vertical", "ratio": 0.5,
                "first": {"window": "window-1"}, "second": {"window": "window-2"},
            }

            class FailingHyprland(FakeHyprland):
                def __init__(self):
                    super().__init__()
                    self.locations = {"0x1": "old", "0x2": "old"}
                    self.failed = False

                def move_window(self, address, *, workspace=None, x=None, y=None):
                    super().move_window(address, workspace=workspace, x=x, y=y)
                    if address == "0x2" and workspace == "1" and not self.failed:
                        self.failed = True
                        raise HyprlandError("simulated placement failure")
                    if workspace is not None:
                        self.locations[address] = workspace

            hypr = FailingHyprland()
            coordinator = RestoreCoordinator(store(Path(folder)), hypr)
            with self.assertRaises(HyprlandError):
                coordinator._rebuild_tiled(value, {"window-1": "0x1", "window-2": "0x2"}, "run")
            self.assertEqual(hypr.locations, {"0x1": "1", "0x2": "1"})

    def test_staging_failure_returns_already_staged_windows(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            value = profile()
            second = dict(value["windows"][0], id="window-2")
            value["windows"].append(second)
            value["workspaces"][0]["tree"] = {
                "split": "vertical", "ratio": 0.5,
                "first": {"window": "window-1"}, "second": {"window": "window-2"},
            }

            class FailingStage(FakeHyprland):
                def __init__(self):
                    super().__init__()
                    self.locations = {"0x1": "old", "0x2": "old"}

                def move_window(self, address, *, workspace=None, x=None, y=None):
                    if address == "0x2" and str(workspace).startswith("name:__workspace_setups_stage"):
                        raise HyprlandError("simulated staging failure")
                    super().move_window(address, workspace=workspace, x=x, y=y)
                    if workspace is not None:
                        self.locations[address] = workspace

            hypr = FailingStage()
            coordinator = RestoreCoordinator(store(Path(folder)), hypr)
            with self.assertRaises(HyprlandError):
                coordinator._rebuild_tiled(value, {"window-1": "0x1", "window-2": "0x2"}, "run")
            self.assertEqual(hypr.locations["0x1"], "1")
            self.assertEqual(hypr.locations["0x2"], "old")

    def test_final_state_failure_is_partial_and_other_windows_continue(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            value = profile()
            second = dict(value["windows"][0], id="window-2", titleHint="Second")
            value["windows"].append(second)
            value["workspaces"][0]["tree"] = {
                "split": "vertical", "ratio": 0.5,
                "first": {"window": "window-1"}, "second": {"window": "window-2"},
            }

            class ResizeFailure(FakeHyprland):
                def resize_window(self, address, width, height):
                    super().resize_window(address, width, height)
                    if address == "0x1":
                        raise HyprlandError("simulated resize failure")

            hypr = ResizeFailure()
            launched = 0

            def launch(*args, **kwargs):
                nonlocal launched
                launched += 1
                hypr.clients.append(client(f"0x{launched}", "Terminal" if launched == 1 else "Second"))
                return Process()

            with patch("workspace_setups.restore.shutil.which", return_value="/usr/bin/foot"), patch(
                "workspace_setups.restore.process_ancestors", return_value={99}
            ):
                result = RestoreCoordinator(store(Path(folder)), hypr, popen=launch).restore(value)
            self.assertEqual(result["state"], "partial")
            self.assertEqual(result["windows"][0]["state"], "failed")
            self.assertIn(("resize_window", "0x2:900x700"), hypr.dispatched)
