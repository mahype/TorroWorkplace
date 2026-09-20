import unittest

from workspace_setups.layout import infer_tree


def window(identifier: str, x: int, y: int, width: int, height: int) -> dict:
    return {"id": identifier, "floating": False, "geometry": {"x": x, "y": y, "width": width, "height": height}}


class LayoutTests(unittest.TestCase):
    def test_vertical_split_ratio(self) -> None:
        tree = infer_tree([window("a", 0, 0, 300, 600), window("b", 300, 0, 700, 600)])
        self.assertEqual(tree["split"], "vertical")
        self.assertEqual(tree["ratio"], 0.3)

    def test_nested_split(self) -> None:
        tree = infer_tree([window("a", 0, 0, 500, 1000), window("b", 500, 0, 500, 400), window("c", 500, 400, 500, 600)])
        self.assertEqual(tree["split"], "vertical")
        self.assertEqual(tree["second"]["split"], "horizontal")

    def test_overlap_uses_fallback(self) -> None:
        tree = infer_tree([window("a", 0, 0, 500, 500), window("b", 100, 100, 500, 500)])
        self.assertIn("fallback", tree)

    def test_floating_is_excluded(self) -> None:
        value = window("a", 0, 0, 1, 1)
        value["floating"] = True
        self.assertEqual(infer_tree([value]), {})
