import io
import json
import os
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

from workspace_setups.capture import capture
from workspace_setups.cli import main

from .helpers import FakeHyprland, client, store, template


def run(arguments, backend, hypr):
    stdout, stderr = io.StringIO(), io.StringIO()
    with redirect_stdout(stdout), redirect_stderr(stderr):
        code = main(arguments, backend, hypr)
    value = stdout.getvalue() or stderr.getvalue()
    return code, json.loads(value) if value else None


class CaptureTests(unittest.TestCase):
    def test_capture_takes_only_the_active_workspace(self) -> None:
        hypr = FakeHyprland([
            dict(client("0x1", "Unknown", cls="missing-app"), fullscreen=1, at=[10, 20], size=[300, 400]),
            client("0x2", workspace=2),
            client("0x3", cls="xdg-desktop-portal-gtk"),
        ])
        hypr.active = {"id": 1, "name": "1", "monitor": "DP-1"}
        value = capture("Demo", hypr)
        self.assertEqual(value["id"], "demo")
        self.assertEqual(len(value["windows"]), 1)
        self.assertFalse(value["windows"][0]["restorable"])
        self.assertEqual(value["windows"][0]["fullscreenMode"], "maximized")
        self.assertEqual(value["windows"][0]["strategy"], "auto")
        self.assertEqual(value["tree"], {"window": "window-1"})
        self.assertEqual((value["source"]["workspace"], value["startupWorkspace"]), ("1", "1"))

    def test_capture_stores_geometry_relative_to_the_monitor(self) -> None:
        hypr = FakeHyprland([dict(client(), at=[2000, 30], floating=True)])
        hypr.monitors[0].update(x=1920)
        self.assertEqual(capture("Demo", hypr)["windows"][0]["geometry"]["x"], 80)

    def test_update_keeps_settings_and_chosen_strategies(self) -> None:
        hypr = FakeHyprland([client()])
        existing = template()
        existing.update(shortcut=4, restoreOnStartup=True, startupWorkspace="7")
        existing["windows"][0]["strategy"] = "move"
        value = capture("Test", hypr, existing)
        self.assertEqual((value["shortcut"], value["restoreOnStartup"], value["startupWorkspace"]), (4, True, "7"))
        self.assertEqual(value["windows"][0]["strategy"], "move")


class CliTests(unittest.TestCase):
    def setUp(self) -> None:
        self.folder = tempfile.TemporaryDirectory()
        self.backend = store(Path(self.folder.name))
        self.hypr = FakeHyprland()
        bindings = patch("workspace_setups.cli.bindings.installed", return_value=False)
        bindings.start()
        self.addCleanup(bindings.stop)

    def tearDown(self) -> None:
        self.folder.cleanup()

    def test_list_shape(self) -> None:
        self.backend.save(template())
        code, value = run(["list", "--json"], self.backend, self.hypr)
        self.assertEqual(code, 0)
        self.assertEqual(value["templates"][0]["id"], "test")
        self.assertEqual((value["language"], value["bindingsInstalled"]), ("auto", False))

    def test_save_refuses_existing_name_with_choice_code(self) -> None:
        self.backend.save(template())
        code, value = run(["save", "Test"], self.backend, self.hypr)
        self.assertEqual((code, value["code"]), (3, "exists"))

    def test_save_slot_creates_then_warns_then_overwrites(self) -> None:
        code, value = run(["save-slot", "F2"], self.backend, self.hypr)
        self.assertEqual((code, value["name"], value["shortcut"]), (0, "Template F2", 2))
        code, value = run(["save-slot", "2"], self.backend, self.hypr)
        self.assertEqual((code, value["code"], value["template"]), (3, "slot_taken", "Template F2"))
        self.backend.rename("template-f2", "Mail")
        code, value = run(["save-slot", "2", "--force"], self.backend, self.hypr)
        self.assertEqual((code, value["name"], value["shortcut"]), (0, "Mail", 2))

    def test_load_slot_reports_empty_slot_and_non_empty_workspace(self) -> None:
        code, value = run(["load-slot", "F5"], self.backend, self.hypr)
        self.assertEqual((code, value["code"]), (2, "slot_empty"))
        self.backend.save(dict(template(), shortcut=5))
        self.hypr.clients.append(client())
        code, value = run(["load-slot", "F5"], self.backend, self.hypr)
        self.assertEqual((code, value["code"], value["workspace"]), (3, "workspace_not_empty", "1"))

    def test_settings_commands(self) -> None:
        self.backend.save(template())
        code, value = run(["set-startup", "test", "on", "--workspace", "4"], self.backend, self.hypr)
        self.assertEqual((code, value["restoreOnStartup"], value["startupWorkspace"]), (0, True, "4"))
        code, value = run(["set-shortcut", "test", "F10"], self.backend, self.hypr)
        self.assertEqual(value["shortcut"], 10)
        code, value = run(["set-shortcut", "test", "none"], self.backend, self.hypr)
        self.assertIsNone(value["shortcut"])
        code, value = run(["set-strategy", "test", "window-1", "move"], self.backend, self.hypr)
        self.assertEqual(value["windows"][0]["strategy"], "move")
        code, value = run(["set-urls", "test", "window-1", "--", "example.test"], self.backend, self.hypr)
        self.assertEqual((code, value["error"]), (2, "not a browser window: window-1"))
        browser = template("Web")
        browser["windows"][0].update(application="chromium", initialClass="chromium", command=["chromium"], **{"class": "chromium"})
        self.backend.save(browser)
        code, value = run(["set-urls", "web", "window-1", "--", "example.test", "https://b.test/x"], self.backend, self.hypr)
        self.assertEqual(value["windows"][0]["urls"], ["https://example.test", "https://b.test/x"])
        self.assertTrue(value["windows"][0]["browser"])
        code, value = run(["set-urls", "web", "window-1", "--", "javascript:alert(1)"], self.backend, self.hypr)
        self.assertEqual(code, 2)
        self.assertEqual(self.backend.get("web")["windows"][0]["urls"], ["https://example.test", "https://b.test/x"])
        code, value = run(["set-language", "de"], self.backend, self.hypr)
        self.assertEqual(self.backend.config()["language"], "de")
        with redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                main(["set-shortcut", "test", "F11"], self.backend, self.hypr)

    def test_startup_runs_once_per_hyprland_instance(self) -> None:
        self.backend.save(dict(template(), restoreOnStartup=True, startupWorkspace="3"))
        restored = []
        with patch.dict(os.environ, {"HYPRLAND_INSTANCE_SIGNATURE": "abc"}), patch(
            "workspace_setups.cli.RestoreCoordinator.restore",
            side_effect=lambda template, mode, workspace, focus: restored.append((mode, workspace)) or {"state": "completed"},
        ):
            code, value = run(["startup"], self.backend, self.hypr)
            self.assertEqual((code, value["skipped"]), (0, False))
            code, value = run(["startup"], self.backend, self.hypr)
            self.assertTrue(value["skipped"])
        self.assertEqual(restored, [("add", "3")])
        self.assertEqual(self.hypr.dispatched[-1], ("focus_workspace", "1"))

    def test_cancel_and_error_json(self) -> None:
        self.backend.paths.state_dir.mkdir(parents=True)
        self.backend.paths.status.write_text(json.dumps({"state": "running", "runId": "run-1", "pid": os.getpid()}))
        self.assertEqual(run(["cancel"], self.backend, self.hypr)[0], 0)
        self.assertTrue(self.backend.paths.cancel.exists())
        code, value = run(["delete", "missing"], self.backend, self.hypr)
        self.assertEqual((code, value["code"]), (2, "not_found"))

    def test_status_does_not_report_dead_restore_as_active(self) -> None:
        self.backend.paths.state_dir.mkdir(parents=True)
        self.backend.paths.status.write_text(json.dumps({"state": "running", "phase": "launching", "pid": 999999999}))
        self.assertEqual(run(["status", "--json"], self.backend, self.hypr)[1]["state"], "failed")


if __name__ == "__main__":
    unittest.main()
