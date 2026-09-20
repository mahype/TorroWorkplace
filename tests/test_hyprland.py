import json
import socket
import subprocess
import tempfile
import unittest
from pathlib import Path

from workspace_setups.hyprland import Hyprland, HyprlandError, normal_clients, window_selector


class Runner:
    def __init__(self) -> None:
        self.commands: list[list[str]] = []

    def __call__(self, command, **kwargs):
        self.commands.append(command)
        stdout = json.dumps([]) if "-j" in command else "ok\n"
        return subprocess.CompletedProcess(command, 0, stdout, "")


class HyprlandTests(unittest.TestCase):
    def test_normal_clients_include_named_but_exclude_special_workspaces(self) -> None:
        common = {"mapped": True, "hidden": False, "class": "foot"}
        named = {**common, "workspace": {"id": -1337, "name": "project"}}
        special = {**common, "workspace": {"id": -99, "name": "special:scratchpad"}}
        self.assertEqual(normal_clients([named, special]), [named])

    def test_selector_normalizes_addresses_and_rejects_code(self) -> None:
        self.assertEqual(window_selector("0xA12"), "address:0xa12")
        self.assertEqual(window_selector("address:a12"), "address:0xa12")
        with self.assertRaises(HyprlandError):
            window_selector('1" }) os.execute("bad")')

    def test_lua_dispatchers_are_explicit_and_address_targeted(self) -> None:
        runner = Runner()
        hypr = Hyprland(run=runner)
        hypr.move_window("0xabc", workspace='name "quoted"')
        hypr.set_floating("abc", True)
        hypr.resize_window("abc", 800, 600)
        hypr.move_window("abc", x=10, y=20)
        hypr.set_fullscreen("abc", "maximized")
        hypr.pin_window("abc")
        hypr.close_window("abc")
        expressions = [command[2] for command in runner.commands]
        self.assertIn('window = "address:0xabc"', expressions[0])
        self.assertIn('workspace = "name \\"quoted\\""', expressions[0])
        self.assertTrue(all(command[:2] == ["hyprctl", "dispatch"] for command in runner.commands))
        self.assertTrue(any("hl.dsp.window.fullscreen" in expression for expression in expressions))

    def test_event_socket_uses_runtime_signature(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            socket_dir = Path(folder) / "hypr" / "signature"
            socket_dir.mkdir(parents=True)
            path = socket_dir / ".socket2.sock"
            server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            server.bind(str(path))
            server.listen(1)
            try:
                client = Hyprland(env={"XDG_RUNTIME_DIR": folder, "HYPRLAND_INSTANCE_SIGNATURE": "signature"}).event_socket()
                accepted, _ = server.accept()
                client.close()
                accepted.close()
            finally:
                server.close()


if __name__ == "__main__":
    unittest.main()
