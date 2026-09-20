import io
import json
import os
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

from workspace_setups.capture import capture
from workspace_setups.cli import main

from .helpers import FakeHyprland, profile, store


class CaptureCliTests(unittest.TestCase):
    def test_capture_models_environment_and_unresolved_window(self) -> None:
        hypr = FakeHyprland([{
            "address": "0x1", "class": "missing-app", "initialClass": "missing-app", "title": "Unknown", "pid": 999999,
            "at": [10, 20], "size": [300, 400], "monitor": "DP-1", "floating": False,
            "fullscreen": 1,
            "mapped": True, "hidden": False, "workspace": {"id": 1, "name": "1"},
        }])
        value = capture("Demo", hypr)
        self.assertEqual(value["id"], "demo")
        self.assertFalse(value["windows"][0]["restorable"])
        self.assertEqual(value["windows"][0]["fullscreenMode"], "maximized")
        self.assertEqual(value["workspaces"][0]["tree"], {"window": "window-1"})
        self.assertEqual(value["environment"]["activeWorkspace"], "1")

        hypr.clients[0]["fullscreen"] = 2
        fullscreen = capture("Demo", hypr)
        self.assertEqual(fullscreen["windows"][0]["fullscreenMode"], "fullscreen")

    def test_cli_list_status_and_startup(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            backend = store(Path(folder))
            backend.save(profile())
            stdout = io.StringIO()
            with redirect_stdout(stdout):
                self.assertEqual(main(["list", "--json"], backend, FakeHyprland()), 0)
            self.assertEqual(json.loads(stdout.getvalue())[0]["id"], "test")
            with redirect_stdout(io.StringIO()):
                self.assertEqual(main(["set-startup", "fixed", "test"], backend, FakeHyprland()), 0)
            self.assertEqual(backend.config()["startupProfile"], "test")
            stdout = io.StringIO()
            with redirect_stdout(stdout):
                self.assertEqual(main(["status", "--json"], backend, FakeHyprland()), 0)
            self.assertEqual(json.loads(stdout.getvalue())["state"], "idle")

    def test_cli_cancel_and_error_json(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            backend = store(Path(folder))
            backend.paths.state_dir.mkdir(parents=True)
            backend.paths.status.write_text(json.dumps({
                "state": "running", "runId": "run-1", "pid": os.getpid()
            }))
            with redirect_stdout(io.StringIO()):
                self.assertEqual(main(["cancel"], backend, FakeHyprland()), 0)
            self.assertTrue(backend.paths.cancel.exists())
            stderr = io.StringIO()
            with redirect_stderr(stderr):
                self.assertEqual(main(["set-startup", "fixed"], backend, FakeHyprland()), 2)
            self.assertIn("error", json.loads(stderr.getvalue()))

    def test_status_does_not_report_dead_restore_as_active(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            backend = store(Path(folder))
            backend.paths.state_dir.mkdir(parents=True)
            backend.paths.status.write_text(json.dumps({
                "state": "running", "phase": "launching", "pid": 999999999
            }))
            stdout = io.StringIO()
            with redirect_stdout(stdout):
                self.assertEqual(main(["status", "--json"], backend, FakeHyprland()), 0)
            self.assertEqual(json.loads(stdout.getvalue())["state"], "failed")
