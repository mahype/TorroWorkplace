import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from workspace_setups.commands import (
    adapt_command,
    default_strategy,
    descendant_cwd,
    desktop_command,
    has_embedded_shell,
    proc_command,
    resolve_command,
    sanitize_command,
    vscode_workspace,
)


class CommandTests(unittest.TestCase):
    def test_sanitizes_placeholders_secrets_and_internal_flags(self) -> None:
        result = sanitize_command(["chromium", "%U", "--token", "secret", "--type=renderer", "--incognito"])
        self.assertEqual(result, ["chromium", "--incognito"])
        self.assertTrue(has_embedded_shell(["foot", "-e", "bash", "-c", "dangerous-command"]))
        self.assertTrue(has_embedded_shell(["foot", "-e", "bash", "-lc", "dangerous-command"]))
        self.assertFalse(has_embedded_shell(["foot", "--working-directory", "/home/user"]))

    def test_desktop_entry_matches_startup_class(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            entry = Path(folder) / "browser.desktop"
            entry.write_text("[Desktop Entry]\nExec=browser %U --safe\nStartupWMClass=Browser\n")
            self.assertEqual(desktop_command("browser", [entry]), ["browser", "--safe"])

    def test_proc_command_reads_nul_arguments(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "42"
            path.mkdir()
            (path / "cmdline").write_bytes(b"foot\0--server\0")
            self.assertEqual(proc_command(42, Path(folder)), ["foot", "--server"])

    def test_single_argument_chromium_pwa_command_is_normalized(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            proc = Path(folder)
            process = proc / "42"
            process.mkdir()
            process.joinpath("cmdline").write_bytes(b"/bin/true --app=https://example.test\0")
            command = resolve_command(
                {"class": "chromium-pwa", "initialClass": "chromium-pwa", "pid": 42},
                proc,
                [],
            )
            self.assertEqual(command, ["/bin/true", "--app=https://example.test"])

    def test_adapters(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            proc = Path(folder)
            process = proc / "7"
            process.mkdir()
            work = Path("/home/test-workspace")
            (process / "cwd").symlink_to(work)
            self.assertEqual(adapt_command({"class": "foot", "pid": 7}, ["foot", "--server"], proc), ["foot", "--working-directory", str(work)])
        self.assertEqual(adapt_command({"class": "Chromium"}, ["chromium", "--incognito"]), ["chromium"])
        self.assertEqual(
            adapt_command({"class": "Chromium"}, ["chromium", "--app-id=mail", "--app=https://mail.test"]),
            ["chromium", "--app-id=mail", "--app=https://mail.test"],
        )
        with patch("workspace_setups.commands.os.path.isdir", return_value=True):
            self.assertEqual(adapt_command({"class": "Code"}, ["code", "/project"]), ["code", "--new-window", "/project"])

    def _process(self, proc: Path, pid: int, parent: int, cmdline: list[str], tpgid: int) -> None:
        entry = proc / str(pid)
        entry.mkdir()
        (entry / "status").write_text(f"PPid:\t{parent}\n")
        (entry / "cmdline").write_bytes(b"\0".join(arg.encode() for arg in cmdline) + b"\0")
        (entry / "stat").write_text(f"{pid} ({Path(cmdline[0]).name}) S {parent} {pid} {pid} 34816 {tpgid} 0\n")

    def test_terminal_keeps_program_started_with_e(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            proc = Path(folder)
            self._process(proc, 7, 1, ["foot", "--working-directory=/home/user", "-e", "herdr"], 0)
            self._process(proc, 8, 7, ["herdr"], 8)
            self.assertEqual(adapt_command({"class": "foot", "pid": 7}, ["foot"], proc), ["foot", "-e", "herdr"])

    def test_terminal_keeps_allowlisted_foreground_program_of_shell(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            proc = Path(folder)
            self._process(proc, 7, 1, ["foot"], 0)
            self._process(proc, 8, 7, ["bash"], 9)
            self._process(proc, 9, 8, ["nvim", "notes.md"], 9)
            self.assertEqual(adapt_command({"class": "foot", "pid": 7}, ["foot"], proc), ["foot", "-e", "nvim", "notes.md"])

    def test_terminal_drops_unknown_or_idle_programs(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            proc = Path(folder)
            self._process(proc, 7, 1, ["foot"], 0)
            self._process(proc, 8, 7, ["bash"], 9)
            self._process(proc, 9, 8, ["ssh", "prod"], 9)
            self._process(proc, 17, 1, ["foot"], 0)
            self._process(proc, 18, 17, ["bash"], 18)
            self.assertEqual(adapt_command({"class": "foot", "pid": 7}, ["foot"], proc), ["foot"])
            self.assertEqual(adapt_command({"class": "foot", "pid": 17}, ["foot"], proc), ["foot"])

    def test_descendant_cwd_prefers_tree_depth_over_pid_number(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            proc = Path(folder)
            for pid, parent in ((10, 1), (99, 10), (20, 10), (21, 20)):
                entry = proc / str(pid)
                entry.mkdir()
                (entry / "status").write_text(f"PPid:\t{parent}\n")
            with patch("workspace_setups.commands.proc_cwd", side_effect=lambda pid, _: {
                99: "/home/shallow", 21: "/home/deep"
            }.get(pid)):
                self.assertEqual(descendant_cwd(10, proc), "/home/deep")

    def test_vscode_workspace_matches_persisted_folder_to_title(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            config = Path(folder) / "Code/User/globalStorage"
            config.mkdir(parents=True)
            project = Path(folder) / "Projects/TorroCast"
            project.mkdir(parents=True)
            (config / "storage.json").write_text(json.dumps({
                "windowsState": {"lastActiveWindow": {"folder": project.as_uri()}, "openedWindows": []}
            }))
            value = vscode_workspace("README - TorroCast - Visual Studio Code", {
                "HOME": folder, "XDG_CONFIG_HOME": folder,
            })
            self.assertEqual(value, str(project))


class StrategyTests(unittest.TestCase):
    def test_defaults(self) -> None:
        self.assertEqual(default_strategy({"class": "foot", "command": ["foot"]}), "new")
        self.assertEqual(default_strategy({"class": "Spotify", "command": ["spotify"]}), "move")
        self.assertEqual(default_strategy({"class": "chrome-mail.test__-Default", "command": ["chromium"]}), "new")
        self.assertEqual(default_strategy({"class": "custom", "command": ["chromium", "--app=https://x.test"]}), "new")
        self.assertEqual(default_strategy({"class": "Code", "command": ["code", "--new-window", "/p"]}), "auto")
