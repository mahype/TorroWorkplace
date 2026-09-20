import json
import tempfile
import unittest
from pathlib import Path

from workspace_setups.paths import Paths
from workspace_setups.storage import StorageError, atomic_json, slugify, validate_profile

from .helpers import profile, store


class StorageTests(unittest.TestCase):
    def test_xdg_and_explicit_overrides(self) -> None:
        value = Paths.from_env({"HOME": "/home/me", "XDG_DATA_HOME": "/xdg/data", "WORKSPACE_SETUPS_STATE_HOME": "/state"})
        self.assertEqual(value.data_dir, Path("/xdg/data/workspace-setups"))
        self.assertEqual(value.state_dir, Path("/state/workspace-setups"))
        self.assertEqual(value.config_dir, Path("/home/me/.config/workspace-setups"))

    def test_slug_rejects_path_and_empty_names(self) -> None:
        self.assertEqual(slugify("My Setup"), "my-setup")
        with self.assertRaises(StorageError):
            slugify("../")

    def test_atomic_write_rejects_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            target = root / "real"
            target.write_text("x")
            (root / "link").symlink_to(target)
            with self.assertRaises(StorageError):
                atomic_json(root / "link", {})

    def test_update_creates_limited_backups(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            backend = store(Path(folder))
            backend.backup_limit = 2
            value = profile()
            backend.save(value)
            for number in range(4):
                value["updatedAt"] = str(number)
                backend.save(value, replace=True)
            backups = list((backend.paths.backups / "test").glob("*.json"))
            self.assertEqual(len(backups), 2)
            self.assertEqual(backend.get("test")["updatedAt"], "3")

    def test_rename_updates_config_and_delete_clears_it(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            backend = store(Path(folder))
            backend.save(profile())
            config = backend.config()
            config.update(startupProfile="test", lastProfile="test")
            backend.write_config(config)
            backend.rename("test", "Work")
            self.assertEqual(backend.config()["startupProfile"], "work")
            backend.delete("work")
            self.assertIsNone(backend.config()["startupProfile"])

    def test_profile_validation_rejects_duplicate_ids_and_bad_commands(self) -> None:
        value = profile()
        value["windows"].append(dict(value["windows"][0]))
        with self.assertRaisesRegex(StorageError, "duplicate"):
            validate_profile(value)
        value = profile()
        value["windows"][0]["command"] = ["foot", "bad\0arg"]
        with self.assertRaisesRegex(StorageError, "command"):
            validate_profile(value)
        value = profile()
        value["windows"][0]["command"] = ["foot", "-e", "bash", "-c", "bad"]
        with self.assertRaisesRegex(StorageError, "embedded shell"):
            validate_profile(value)
        value = profile()
        value["windows"][0]["command"] = ["foot", "-e", "bash", "-lc", "bad"]
        with self.assertRaisesRegex(StorageError, "embedded shell"):
            validate_profile(value)
        value = profile()
        value["windows"][0]["geometry"]["width"] = float("nan")
        with self.assertRaisesRegex(StorageError, "geometry"):
            validate_profile(value)
        value = profile()
        value["windows"][0]["id"] = ["not", "hashable"]
        with self.assertRaisesRegex(StorageError, "invalid window"):
            validate_profile(value)
        value = profile()
        value["workspaces"][0]["tree"]["window"] = "missing"
        with self.assertRaisesRegex(StorageError, "workspace tree"):
            validate_profile(value)
        value = profile()
        value["workspaces"][0]["tree"] = {
            "split": "vertical", "ratio": "bad", "first": {"window": "window-1"}, "second": {"window": "window-1"}
        }
        with self.assertRaisesRegex(StorageError, "ratio"):
            validate_profile(value)

    def test_list_has_ui_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            backend = store(Path(folder))
            backend.save(profile())
            listed = backend.list()[0]
            self.assertEqual((listed["workspaceCount"], listed["windowCount"]), (1, 1))
