#!/usr/bin/env python3
"""Prepare a transparent macOS icon source from the shared app icon."""

from __future__ import annotations

import sys
from collections import deque
from pathlib import Path

from PIL import Image


def is_edge_background(pixel: tuple[int, int, int, int]) -> bool:
    """Treat transparent or baked checkerboard edge pixels as icon background."""
    red, green, blue, alpha = pixel
    if alpha < 8:
        return True
    return (
        alpha == 255
        and red >= 235
        and green >= 235
        and blue >= 235
        and abs(red - green) <= 8
        and abs(green - blue) <= 8
    )


def remove_connected_edge_background(image: Image.Image) -> Image.Image:
    result = image.convert("RGBA")
    pixels = result.load()
    width, height = result.size
    seen = bytearray(width * height)
    queue: deque[tuple[int, int]] = deque()

    def enqueue(x: int, y: int) -> None:
        index = y * width + x
        if seen[index] or not is_edge_background(pixels[x, y]):
            return
        seen[index] = 1
        queue.append((x, y))

    for x in range(width):
        enqueue(x, 0)
        enqueue(x, height - 1)
    for y in range(height):
        enqueue(0, y)
        enqueue(width - 1, y)

    while queue:
        x, y = queue.popleft()
        red, green, blue, _ = pixels[x, y]
        pixels[x, y] = (red, green, blue, 0)
        for next_x, next_y in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
            if 0 <= next_x < width and 0 <= next_y < height:
                enqueue(next_x, next_y)

    return result


ICNS_SIZES = [(16, 16), (32, 32), (64, 64), (128, 128), (256, 256), (512, 512), (1024, 1024)]


def main() -> int:
    if len(sys.argv) not in (3, 4):
        print("usage: prepare-icon.py <source-png> <output-png> [output-icns]", file=sys.stderr)
        return 2

    source = Path(sys.argv[1])
    output = Path(sys.argv[2])
    icns_output = Path(sys.argv[3]) if len(sys.argv) == 4 else None
    if not source.is_file():
        print(f"Missing source icon: {source}", file=sys.stderr)
        return 1

    output.parent.mkdir(parents=True, exist_ok=True)
    prepared = remove_connected_edge_background(Image.open(source))
    prepared.save(output)
    if icns_output:
        icns_output.parent.mkdir(parents=True, exist_ok=True)
        prepared.save(icns_output, format="ICNS", sizes=ICNS_SIZES)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
