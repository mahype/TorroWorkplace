import tempfile
import unittest
from pathlib import Path

from workspace_setups import bindings


class BindingsTests(unittest.TestCase):
    def test_install_appends_guarded_include_once(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder)
            source = home / ".config/omarchy/plugins/io.github.mahype.workspace-setups/hypr/bindings.lua"
            target = home / ".config/hypr/bindings.lua"
            target.parent.mkdir(parents=True)
            target.write_text("-- mine\n")
            env = {"HOME": folder}
            self.assertFalse(bindings.installed(target))
            self.assertTrue(bindings.install(target, source, env))
            self.assertFalse(bindings.install(target, source, env))
            text = target.read_text()
            self.assertTrue(text.startswith("-- mine\n\n" + bindings.MARKER))
            self.assertEqual(text.count(bindings.MARKER), 1)
            self.assertIn('(os.getenv("HOME") or "") .. "/.config/omarchy/plugins/', text)
            self.assertIn("if workspace_setups_file then", text)

    def test_plugin_file_binds_all_slots_through_the_shell(self) -> None:
        text = bindings.PLUGIN_BINDINGS.read_text()
        self.assertIn('"SUPER + F" .. slot', text)
        self.assertIn('"SUPER + SHIFT + F" .. slot', text)
        self.assertIn("omarchy-shell -q workspace-setups loadSlot", text)
        self.assertIn("for slot = 1, 10 do", text)


if __name__ == "__main__":
    unittest.main()
