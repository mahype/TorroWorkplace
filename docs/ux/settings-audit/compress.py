#!/usr/bin/env python3
"""Shrinks the audit's screenshots: flat UI pictures fit into 128 colours with no visible loss at 2x.

usage: python3 compress.py [<dir>]   (needs Pillow; already shrunk files are skipped)
The originals are Retina-size PNGs from `--shots`, about 150 KB each; after this about 55 KB.
"""
import sys
from pathlib import Path

from PIL import Image

root = Path(sys.argv[1] if len(sys.argv) > 1 else Path(__file__).parent / "screenshots")
before = after = 0
for png in sorted(root.rglob("*.png")):
    size = png.stat().st_size
    with Image.open(png) as im:
        if im.mode == "P":  # already a palette picture: done earlier
            continue
        small = im.convert("RGB").quantize(colors=128, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    small.save(png, optimize=True)
    before += size
    after += png.stat().st_size
print(f"{before / 1e6:.1f} MB -> {after / 1e6:.1f} MB")
