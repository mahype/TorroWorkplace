import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from workspace_setups.hyprland import HyprlandError
from workspace_setups.restore import RestoreCoordinator, RestoreError, match_client
from workspace_setups.storage import atomic_json

from .helpers import FakeHyprland, client, store, template


class Process:
    pid = 99


def two_windows() -> dict:
    value = template()
    value["windows"].append(dict(value["windows"][0], id="window-2", titleHint="Second"))
    value["tree"] = {"split": "vertical", "ratio": 0.3,
                     "first": {"window": "window-1"}, "second": {"window": "window-2"}}
    return value


class RestoreTests(unittest.TestCase):
    def setUp(self) -> None:
        self.folder = tempfile.TemporaryDirectory()
        self.backend = store(Path(self.folder.name))
        self.backend.write_config(dict(self.backend.config(), windowTimeoutSeconds=0))
        patches = [
            patch("workspace_setups.restore.shutil.which", return_value="/usr/bin/foot"),
            patch("workspace_setups.restore.process_ancestors", return_value={99}),
        ]
        for item in patches:
            item.start()
            self.addCleanup(item.stop)

    def tearDown(self) -> None:
        self.folder.cleanup()

    def restore(self, hypr, value=None, mode="ask", workspace=None, spawn=None):
        launched = []

        def launch(command, **kwargs):
            launched.append(command)
            if spawn:
                hypr.clients.append(spawn(len(launched)))
            return Process()

        coordinator = RestoreCoordinator(self.backend, hypr, popen=launch)
        return coordinator.restore(value or template(), mode, workspace), launched

    def test_match_uses_class_title_and_used_addresses(self) -> None:
        expected = template()["windows"][0]
        clients = [client("0x1", "Other"), client("0x2", "Terminal project")]
        self.assertEqual(match_client(expected, clients, set())["address"], "0x2")
        self.assertEqual(match_client(expected, clients, {"0x2"})["address"], "0x1")

    def test_match_rejects_wrong_class_even_for_launch_descendant(self) -> None:
        expected = template()["windows"][0]
        self.assertIsNone(match_client(expected, [client(cls="splash")], set(), 99))

    def test_non_empty_workspace_asks_before_touching_anything(self) -> None:
        hypr = FakeHyprland([client()])
        with self.assertRaises(RestoreError) as caught:
            self.restore(hypr)
        self.assertEqual(caught.exception.code, "workspace_not_empty")
        self.assertEqual(caught.exception.details["windows"], 1)
        self.assertEqual(hypr.dispatched, [])

    def test_windows_on_other_workspaces_do_not_block(self) -> None:
        hypr = FakeHyprland([client("0x9", workspace=2)])
        result, launched = self.restore(hypr, spawn=lambda n: client(f"0x{n}"))
        self.assertEqual(result["state"], "completed")
        self.assertEqual(len(launched), 1)

    def test_replace_closes_only_windows_on_the_target_workspace(self) -> None:
        hypr = FakeHyprland([client("0x7"), client("0x8", workspace=2)])
        result, _ = self.restore(hypr, mode="replace", spawn=lambda n: client(f"0x{n}"))
        self.assertIn(("close_window", "0x7"), hypr.dispatched)
        self.assertNotIn(("close_window", "0x8"), hypr.dispatched)
        self.assertEqual(result["state"], "completed")

    def test_add_keeps_existing_windows_and_never_matches_them(self) -> None:
        hypr = FakeHyprland([client("0x7")])
        result, _ = self.restore(hypr, mode="add", spawn=lambda n: client(f"0x{n}"))
        self.assertNotIn(("close_window", "0x7"), hypr.dispatched)
        self.assertEqual(result["windows"][0]["address"], "0x1")

    def test_new_strategy_launches_even_if_the_app_runs_elsewhere(self) -> None:
        hypr = FakeHyprland([client("0x7", workspace=3)])
        result, launched = self.restore(hypr, spawn=lambda n: client(f"0x{n}"))
        self.assertEqual(len(launched), 1)
        self.assertEqual(result["windows"][0], {"id": "window-1", "application": "foot",
                                                "state": "restored", "address": "0x1"})
        self.assertEqual(hypr.clients[0]["workspace"]["name"], "3")

    def test_browser_window_opens_its_tabs_in_a_new_window(self) -> None:
        value = template()
        value["windows"][0].update(application="chromium", initialClass="chromium", command=["chromium"],
                                   urls=["https://a.test/", "https://b.test/"], **{"class": "chromium"})
        result, launched = self.restore(FakeHyprland(), value, spawn=lambda n: client(f"0x{n}", cls="chromium"))
        self.assertEqual(launched[0][-4:], ["chromium", "--new-window", "https://a.test/", "https://b.test/"])
        self.assertEqual(result["windows"][0]["state"], "restored")

    def test_move_strategy_pulls_window_from_other_workspace(self) -> None:
        value = template()
        value["windows"][0].update(strategy="move", application="Spotify", initialClass="Spotify", **{"class": "Spotify"})
        hypr = FakeHyprland([client("0x7", "Spotify", workspace=3, cls="Spotify")])
        result, launched = self.restore(hypr, value)
        self.assertEqual(launched, [])
        self.assertEqual(result["windows"][0]["state"], "moved")
        self.assertIn(("move_window", "0x7:1"), hypr.dispatched)
        self.assertEqual(hypr.clients[0]["workspace"]["name"], "1")

    def test_move_strategy_launches_when_nothing_is_open(self) -> None:
        value = template()
        value["windows"][0]["strategy"] = "move"
        hypr = FakeHyprland()
        result, launched = self.restore(hypr, value, spawn=lambda n: client(f"0x{n}"))
        self.assertEqual(len(launched), 1)
        self.assertEqual(result["windows"][0]["state"], "restored")

    def test_auto_strategy_falls_back_to_moving_the_running_window(self) -> None:
        value = template()
        value["windows"][0]["strategy"] = "auto"
        hypr = FakeHyprland([client("0x7", workspace=4)])
        result, launched = self.restore(hypr, value)
        self.assertEqual(len(launched), 1)
        self.assertEqual(result["windows"][0]["state"], "moved")
        self.assertEqual(hypr.clients[0]["workspace"]["name"], "1")

    def test_auto_strategy_prefers_a_fresh_window(self) -> None:
        value = template()
        value["windows"][0]["strategy"] = "auto"
        hypr = FakeHyprland([client("0x7", workspace=4)])
        result, _ = self.restore(hypr, value, spawn=lambda n: client(f"0x{n}"))
        self.assertEqual(result["windows"][0]["address"], "0x1")
        self.assertEqual(hypr.clients[0]["workspace"]["name"], "4")

    def test_explicit_workspace_is_the_target(self) -> None:
        hypr = FakeHyprland([client("0x9", workspace=1)])
        result, _ = self.restore(hypr, workspace="5", spawn=lambda n: client(f"0x{n}", workspace=1))
        self.assertEqual(result["workspace"], "5")
        self.assertIn(("move_window", "0x1:5"), hypr.dispatched)
        self.assertEqual(hypr.dispatched[-1], ("focus_workspace", "5"))

    def test_named_workspace_uses_hyprland_name_selector(self) -> None:
        hypr = FakeHyprland()
        hypr.active = {"id": -1337, "name": "project", "monitor": "DP-1"}
        result, _ = self.restore(hypr, spawn=lambda n: dict(client(f"0x{n}"), workspace={"id": 5, "name": "5"}))
        self.assertEqual(result["state"], "completed")
        self.assertIn(("move_window", "0x1:name:project"), hypr.dispatched)
        self.assertEqual(hypr.dispatched[-1], ("focus_workspace", "name:project"))

    def test_tree_drives_dwindle_preselection_and_ratio(self) -> None:
        hypr = FakeHyprland()
        result, _ = self.restore(hypr, two_windows(),
                                 spawn=lambda n: client(f"0x{n}", "Terminal" if n == 1 else "Second"))
        self.assertEqual(result["state"], "completed")
        self.assertIn(("focus_window", "0x1"), hypr.dispatched)
        self.assertIn(("layout", "preselect r"), hypr.dispatched)
        self.assertIn(("layout", "splitratio 0.6000 exact"), hypr.dispatched)

    def test_floating_geometry_is_relative_to_the_target_monitor(self) -> None:
        value = template()
        value["windows"][0].update(floating=True, geometry={"x": 100, "y": 50, "width": 400, "height": 300})
        value["tree"] = {}
        hypr = FakeHyprland()
        hypr.monitors[0].update(x=1920, y=0)
        self.restore(hypr, value, spawn=lambda n: client(f"0x{n}"))
        self.assertIn(("move_window", "0x1:2020,50"), hypr.dispatched)
        self.assertIn(("resize_window", "0x1:400x300"), hypr.dispatched)

    def test_tiled_sizes_are_only_forced_on_the_same_monitor_size(self) -> None:
        hypr = FakeHyprland()
        hypr.monitors[0]["width"] = 2560
        self.restore(hypr, spawn=lambda n: client(f"0x{n}"))
        self.assertFalse([item for item in hypr.dispatched if item[0] == "resize_window"])

    def test_balanced_tree_uses_parent_first_insertion_plan(self) -> None:
        value = template()
        value["windows"] = [dict(value["windows"][0], id=identifier) for identifier in ("a", "b", "c", "d")]
        value["tree"] = {
            "split": "vertical", "ratio": 0.5,
            "first": {"split": "horizontal", "ratio": 0.4, "first": {"window": "a"}, "second": {"window": "b"}},
            "second": {"split": "horizontal", "ratio": 0.6, "first": {"window": "c"}, "second": {"window": "d"}},
        }
        coordinator = RestoreCoordinator(self.backend, FakeHyprland())
        self.assertEqual([window["id"] for window in coordinator._ordered_windows(value)], ["a", "c", "b", "d"])
        _, operations = coordinator._tree_plan(value["tree"])
        self.assertEqual(operations, [("c", "a", "r", 0.5), ("b", "a", "d", 0.4), ("d", "c", "d", 0.6)])

    def test_old_cancel_marker_is_ignored_and_cleared(self) -> None:
        self.backend.paths.state_dir.mkdir(parents=True, exist_ok=True)
        self.backend.paths.cancel.write_text("old")
        value = template()
        value["windows"], value["tree"] = [], {}
        result, _ = self.restore(FakeHyprland(), value)
        self.assertEqual(result["state"], "completed")
        self.assertFalse(self.backend.paths.cancel.exists())

    def test_current_run_cancellation_stops_before_launch(self) -> None:
        class CancellingCoordinator(RestoreCoordinator):
            cancelled = False

            def _status(self, status):
                super()._status(status)
                if status.get("state") == "running" and not self.cancelled:
                    self.cancelled = True
                    atomic_json(self.paths.cancel, {"runId": status["runId"]})

        coordinator = CancellingCoordinator(
            self.backend, FakeHyprland(), popen=lambda *args, **kwargs: self.fail("application was launched")
        )
        self.assertEqual(coordinator.restore(template())["state"], "cancelled")

    def test_rebuild_failure_moves_staged_windows_back(self) -> None:
        class FailingHyprland(FakeHyprland):
            failed = False

            def move_window(self, address, *, workspace=None, x=None, y=None):
                super().move_window(address, workspace=workspace, x=x, y=y)
                if address == "0x2" and workspace == "1" and not self.failed:
                    self.failed = True
                    raise HyprlandError("simulated placement failure")

        hypr = FailingHyprland([client("0x1", workspace=4), client("0x2", workspace=4)])
        coordinator = RestoreCoordinator(self.backend, hypr)
        target = {"id": 1, "name": "1", "selector": "1"}
        with self.assertRaises(HyprlandError):
            coordinator._rebuild_tiled(two_windows(), {"window-1": "0x1", "window-2": "0x2"}, target, "run")
        self.assertEqual([item["workspace"]["name"] for item in hypr.clients], ["1", "1"])

    def test_final_state_failure_is_partial_and_other_windows_continue(self) -> None:
        class ResizeFailure(FakeHyprland):
            def resize_window(self, address, width, height):
                super().resize_window(address, width, height)
                if address == "0x1":
                    raise HyprlandError("simulated resize failure")

        hypr = ResizeFailure()
        result, _ = self.restore(hypr, two_windows(),
                                 spawn=lambda n: client(f"0x{n}", "Terminal" if n == 1 else "Second"))
        self.assertEqual(result["state"], "partial")
        self.assertEqual(result["windows"][0]["state"], "failed")
        self.assertIn(("resize_window", "0x2:900x700"), hypr.dispatched)


if __name__ == "__main__":
    unittest.main()
