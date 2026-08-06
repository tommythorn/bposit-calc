#!/usr/bin/env python3
"""Generate the app icons.

Kept as a script rather than hand-drawn binaries so the icon can be regenerated if the field
colours in style.css ever change. Writes PNGs with zlib directly — no image library needed.
"""

import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "www" / "icons"

BG = (0x12, 0x15, 0x1C)
PANEL = (0x0D, 0x15, 0x10)

# sign, regime, terminator, exponent, fraction — matching style.css.
FIELDS = [
    ((0xFF, 0x6B, 0x6B), 1),
    ((0xFF, 0xD1, 0x66), 3),
    ((0xFF, 0x9F, 0x45), 1),
    ((0xE8, 0x79, 0xF9), 3),
    ((0x5E, 0xE6, 0xA8), 8),
]


def render(size: int) -> bytes:
    """An LCD panel showing one posit's bit fields, as coloured cells."""
    px = [[BG for _ in range(size)] for _ in range(size)]

    pad = round(size * 0.11)
    inner = size - 2 * pad
    for y in range(pad, pad + inner):
        for x in range(pad, pad + inner):
            px[y][x] = PANEL

    total = sum(w for _, w in FIELDS)
    gap = max(1, size // 90)
    cell = (inner - gap * (total - 1)) / total

    # Two stacked bars: the bounded reading on top, a wider regime below it to hint at the cap.
    bar_h = round(inner * 0.26)
    y0 = pad + round(inner * 0.16)
    y1 = pad + inner - round(inner * 0.16) - bar_h

    def bar(top: int, widths):
        x = pad
        for colour, w in widths:
            span = round(w * cell + (w - 1) * gap)
            for yy in range(top, min(top + bar_h, size)):
                for xx in range(x, min(x + span, pad + inner)):
                    px[yy][xx] = colour
            x += span + gap

    bar(y0, FIELDS)
    # Below: the regime has eaten the terminator, so fewer fraction bits remain.
    bar(y1, [(FIELDS[0][0], 1), (FIELDS[1][0], 6), (FIELDS[3][0], 3), (FIELDS[4][0], 6)])

    raw = b"".join(
        b"\x00" + b"".join(struct.pack("3B", *px[y][x]) for x in range(size))
        for y in range(size)
    )

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for size in (180, 192, 512):
        path = OUT / f"icon-{size}.png"
        path.write_bytes(render(size))
        print(f"wrote {path} ({path.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
