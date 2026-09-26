import json
import tempfile
import unittest
from pathlib import Path

from workspace_setups.paths import Paths
from workspace_setups.storage import StorageError, atomic_json, slugify, validate_template

from .helpers import store, template


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
            value = template()
            backend.save(value)
            for number in range(4):
                value["updatedAt"] = str(number)
                backend.save(value, replace=True)
            backups = list((backend.paths.backups / "test").glob("*.json"))
            self.assertEqual(len(backups), 2)
            self.assertEqual(backend.get("test")["updatedAt"], "3")

    def test_rename_moves_file_and_keeps_settings(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            backend = store(Path(folder))
            value = template()
            value["shortcut"] = 3
            backend.save(value)
            backend.rename("test", "Work")
            self.assertFalse(backend.template_path("test").exists())
            self.assertEqual(backend.get("work")["shortcut"], 3)
            backend.delete("work")
            self.assertEqual(backend.list(), [])

    def test_shortcut_slots_are_unique(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            backend = store(Path(folder))
            backend.save(template("One"))
            backend.save(template("Two"))
            backend.assign_shortcut("one", 2)
            with self.assertRaises(StorageError) as caught:
                backend.assign_shortcut("two", 2)
            self.assertEqual((caught.exception.code, caught.exception.details["template"]), ("slot_taken", "One"))
            backend.assign_shortcut("two", 2, force=True)
            self.assertIsNone(backend.get("one")["shortcut"])
            self.assertEqual(backend.by_slot(2)["id"], "two")
            with self.assertRaisesRegex(StorageError, "shortcut"):
                backend.assign_shortcut("two", 11)

    def test_startup_requires_a_numbered_workspace(self) -> None:
        value = template()
        value.update(restoreOnStartup=True, startupWorkspace=None)
        with self.assertRaisesRegex(StorageError, "startup workspace"):
            validate_template(value)
        value["startupWorkspace"] = "special"
        with self.assertRaisesRegex(StorageError, "workspace number"):
            validate_template(value)
        value["startupWorkspace"] = "4"
        validate_template(value)

    def test_old_profiles_are_not_templates(self) -> None:
        value = template()
        value["schemaVersion"] = 1
        with self.assertRaisesRegex(StorageError, "schema"):
            validate_template(value)

    def test_profile_validation_rejects_duplicate_ids_and_bad_commands(self) -> None:
        value = template()
        value["windows"].append(dict(value["windows"][0]))
        with self.assertRaisesRegex(StorageError, "duplicate"):
            validate_template(value)
        value = template()
        value["windows"][0]["command"] = ["foot", "bad\0arg"]
        with self.assertRaisesRegex(StorageError, "command"):
            validate_template(value)
        value = template()
        value["windows"][0]["command"] = ["foot", "-e", "bash", "-c", "bad"]
        with self.assertRaisesRegex(StorageError, "embedded shell"):
            validate_template(value)
        value = template()
        value["windows"][0]["command"] = ["foot", "-e", "bash", "-lc", "bad"]
        with self.assertRaisesRegex(StorageError, "embedded shell"):
            validate_template(value)
        value = template()
        value["windows"][0]["geometry"]["width"] = float("nan")
        with self.assertRaisesRegex(StorageError, "geometry"):
            validate_template(value)
        value = template()
        value["windows"][0]["strategy"] = "teleport"
        with self.assertRaisesRegex(StorageError, "strategy"):
            validate_template(value)
        value = template()
        value["windows"][0]["id"] = ["not", "hashable"]
        with self.assertRaisesRegex(StorageError, "invalid window"):
            validate_template(value)
        value = template()
        value["tree"]["window"] = "missing"
        with self.assertRaisesRegex(StorageError, "workspace tree"):
            validate_template(value)
        value = template()
        value["tree"] = {
            "split": "vertical", "ratio": "bad", "first": {"window": "window-1"}, "second": {"window": "window-1"}
        }
        with self.assertRaisesRegex(StorageError, "ratio"):
            validate_template(value)

    def test_list_has_ui_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            backend = store(Path(folder))
            backend.save(template())
            listed = backend.list()[0]
            self.assertEqual((listed["windowCount"], listed["shortcut"], listed["restoreOnStartup"]), (1, None, False))
            self.assertEqual(listed["windows"][0]["strategy"], "new")
