import struct
import tempfile
import unittest
from pathlib import Path

from workspace_setups.browser import attach_tabs, read_session, restorable_url


def _string(value: str, width: int = 1) -> bytes:
    raw = value.encode("utf-16-le" if width == 2 else "utf-8")
    return struct.pack("<i", len(value)) + raw + b"\0" * (-len(raw) % 4)


def _record(command: int, payload: bytes) -> bytes:
    return struct.pack("<H", len(payload) + 1) + bytes([command]) + payload


def _navigation(tab: int, index: int, url: str, title: str) -> bytes:
    body = struct.pack("<ii", tab, index) + _string(url) + _string(title, 2)
    return _record(6, struct.pack("<I", len(body)) + body)


def session(*records: bytes) -> bytes:
    return b"SNSS" + struct.pack("<i", 3) + b"".join(records)


def two_windows() -> bytes:
    return session(
        _record(9, struct.pack("<ii", 1, 0)), _record(9, struct.pack("<ii", 2, 0)),
        _record(0, struct.pack("<ii", 1, 10)), _record(0, struct.pack("<ii", 1, 11)),
        _record(0, struct.pack("<ii", 2, 20)),
        _record(2, struct.pack("<ii", 10, 1)), _record(2, struct.pack("<ii", 11, 0)),
        _navigation(10, 0, "https://old.test/", "Old"), _navigation(10, 1, "https://docs.test/", "Docs"),
        _record(7, struct.pack("<ii", 10, 1)),
        _navigation(11, 0, "https://mail.test/", "Mail"), _record(7, struct.pack("<ii", 11, 0)),
        _record(8, struct.pack("<ii", 1, 1)),
        _navigation(20, 0, "chrome://newtab/", "New Tab"), _record(7, struct.pack("<ii", 20, 0)),
    )


class BrowserTests(unittest.TestCase):
    def test_replays_tabs_in_order_with_current_navigation(self) -> None:
        windows = read_session(two_windows())
        self.assertEqual(windows[0], {"urls": ["https://mail.test/", "https://docs.test/"], "title": "Docs"})
        self.assertEqual(windows[1], {"urls": [], "title": "New Tab"})

    def test_closed_tabs_and_windows_are_dropped(self) -> None:
        data = two_windows() + _record(16, struct.pack("<iq", 11, 0)) + _record(17, struct.pack("<iq", 2, 0))
        self.assertEqual(read_session(data), [{"urls": ["https://docs.test/"], "title": "Docs"}])

    def test_rejects_non_web_urls(self) -> None:
        self.assertTrue(restorable_url("https://example.test/?q=1"))
        for url in ("chrome://settings", "javascript:alert(1)", "--incognito", "", "https://a.test/\nx"):
            self.assertFalse(restorable_url(url))

    def test_attaches_tabs_by_active_tab_title(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            sessions = Path(folder) / "chromium/Default/Sessions"
            sessions.mkdir(parents=True)
            (sessions / "Session_1").write_bytes(two_windows())
            windows = [
                {"class": "chromium", "command": ["chromium"], "titleHint": "New Tab - Chromium"},
                {"class": "chromium", "command": ["chromium"], "titleHint": "Docs - Chromium"},
                {"class": "chrome-mail", "command": ["chromium", "--app=https://mail.test"], "titleHint": "Docs"},
            ]
            attach_tabs(windows, {"HOME": folder, "XDG_CONFIG_HOME": folder})
            self.assertNotIn("urls", windows[0])
            self.assertEqual(windows[1]["urls"], ["https://mail.test/", "https://docs.test/"])
            self.assertNotIn("urls", windows[2])


if __name__ == "__main__":
    unittest.main()
