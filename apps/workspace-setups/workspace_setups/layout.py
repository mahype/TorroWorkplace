from __future__ import annotations

from typing import Any


def infer_tree(windows: list[dict[str, Any]]) -> dict[str, Any]:
    """Infer a binary tiling tree from non-overlapping client rectangles."""
    items = [(window["id"], _rect(window["geometry"])) for window in windows if not window.get("floating")]
    if not items:
        return {}
    return _split(items)


def _rect(value: dict[str, Any]) -> tuple[float, float, float, float]:
    return (float(value["x"]), float(value["y"]), float(value["width"]), float(value["height"]))


def _split(items: list[tuple[str, tuple[float, float, float, float]]]) -> dict[str, Any]:
    if len(items) == 1:
        return {"window": items[0][0]}
    candidates: list[tuple[float, str, list[Any], list[Any], float]] = []
    for direction, axis, extent in (("vertical", 0, 2), ("horizontal", 1, 3)):
        edges = sorted({rect[axis] + rect[extent] for _, rect in items})
        for edge in edges:
            first = [item for item in items if item[1][axis] + item[1][extent] <= edge + 1]
            second = [item for item in items if item[1][axis] >= edge - 1 and item not in first]
            if first and second and len(first) + len(second) == len(items):
                start = min(rect[axis] for _, rect in items)
                end = max(rect[axis] + rect[extent] for _, rect in items)
                boundary = (max(rect[axis] + rect[extent] for _, rect in first) + min(rect[axis] for _, rect in second)) / 2
                ratio = (boundary - start) / (end - start) if end > start else 0.5
                candidates.append((abs(len(first) - len(second)), direction, first, second, ratio))
    if not candidates:
        return {"fallback": [identifier for identifier, _ in sorted(items)]}
    _, direction, first, second, ratio = min(candidates, key=lambda item: (item[0], item[1]))
    return {"split": direction, "ratio": round(ratio, 4), "first": _split(first), "second": _split(second)}
