#!/usr/bin/env python3
"""Generate Aurora TV's application icons.

tauri-build needs `icons/icon.ico` to embed a Windows resource. Rather than commit an
opaque binary, this script draws the mark from scratch — the same rounded square and
"A" the app shows in its sidebar, in the accent-to-accent-2 gradient from
src-ui/src/styles/tokens.css — and writes both the PNG set and a 32bpp BMP-backed ICO.

Pure stdlib: no Pillow, no ImageMagick. Run it from the repo root:

    python3 tools/make_icons.py
"""
from __future__ import annotations

import struct
import zlib
from pathlib import Path

OUT = Path("src-native/crates/aurora-app/icons")

# --- tokens.css: --accent #7c5cff, --accent-2 #2dd4bf -------------------------
ACCENT = (0x7C, 0x5C, 0xFF)
ACCENT_2 = (0x2D, 0xD4, 0xBF)
GLYPH = (0xFF, 0xFF, 0xFF)

SS = 4  # supersampling factor for antialiasing


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def inside_triangle(px, py, ax, ay, bx, by, cx, cy) -> bool:
    """Barycentric sign test."""
    d1 = (px - bx) * (ay - by) - (ax - bx) * (py - by)
    d2 = (px - cx) * (by - cy) - (bx - cx) * (py - cy)
    d3 = (px - ax) * (cy - ay) - (cx - ax) * (py - ay)
    has_neg = (d1 < 0) or (d2 < 0) or (d3 < 0)
    has_pos = (d1 > 0) or (d2 > 0) or (d3 > 0)
    return not (has_neg and has_pos)


def inside_rounded_rect(px, py, size, radius) -> bool:
    x = min(px, size - px)
    y = min(py, size - py)
    if x < 0 or y < 0:
        return False
    if x >= radius or y >= radius:
        return True
    dx, dy = radius - x, radius - y
    return dx * dx + dy * dy <= radius * radius


def render(size: int) -> bytes:
    """Draw the icon at `size` px, returning RGBA bytes."""
    s = size * SS
    radius = 0.22 * s

    # Glyph geometry, all relative to the supersampled canvas.
    cx = s / 2
    y0, y1 = 0.20 * s, 0.80 * s
    half_w = 0.30 * s
    leg = 0.155 * s

    outer = (cx, y0, cx - half_w, y1, cx + half_w, y1)
    inner = (cx, y0 + 0.26 * s, cx - half_w + leg, y1, cx + half_w - leg, y1)

    bar_top, bar_bottom = 0.615 * s, 0.715 * s

    # Accumulate at supersampled resolution, then box-downsample.
    acc = [[0.0, 0.0, 0.0, 0.0] for _ in range(size * size)]

    for sy in range(s):
        py = sy + 0.5
        # The crossbar spans between the outer edges at this height, inset a little.
        for sx in range(s):
            px = sx + 0.5
            if not inside_rounded_rect(px, py, s, radius):
                continue

            # Diagonal gradient, matching the CSS 135deg.
            t = ((px / s) + (py / s)) / 2
            r = lerp(ACCENT[0], ACCENT_2[0], t)
            g = lerp(ACCENT[1], ACCENT_2[1], t)
            b = lerp(ACCENT[2], ACCENT_2[2], t)

            in_legs = inside_triangle(px, py, *outer) and not inside_triangle(px, py, *inner)
            in_bar = bar_top <= py <= bar_bottom and inside_triangle(px, py, *outer)
            if in_legs or in_bar:
                r, g, b = GLYPH

            cell = acc[(sy // SS) * size + (sx // SS)]
            cell[0] += r
            cell[1] += g
            cell[2] += b
            cell[3] += 255.0

    n = SS * SS
    out = bytearray(size * size * 4)
    for i, (r, g, b, a) in enumerate(acc):
        alpha = a / n
        if alpha <= 0:
            continue
        # Un-premultiply: colour is the average over covered subsamples only.
        cov = a / 255.0
        out[i * 4 + 0] = min(255, round(r / cov))
        out[i * 4 + 1] = min(255, round(g / cov))
        out[i * 4 + 2] = min(255, round(b / cov))
        out[i * 4 + 3] = round(alpha)
    return bytes(out)


def write_png(path: Path, rgba: bytes, size: int) -> None:
    raw = bytearray()
    for y in range(size):
        raw.append(0)  # filter type 0
        raw += rgba[y * size * 4 : (y + 1) * size * 4]

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)


def write_ico(path: Path, images: list[tuple[int, bytes]]) -> None:
    """Classic BMP-backed ICO. Widely compatible, including with tauri-build."""
    entries, payloads, offset = [], [], 6 + 16 * len(images)

    for size, rgba in images:
        # BITMAPINFOHEADER: height is doubled to cover the (unused) AND mask.
        header = struct.pack(
            "<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, size * size * 4, 0, 0, 0, 0
        )
        # BGRA, bottom-up.
        pixels = bytearray()
        for y in range(size - 1, -1, -1):
            row = rgba[y * size * 4 : (y + 1) * size * 4]
            for x in range(size):
                r, g, b, a = row[x * 4 : x * 4 + 4]
                pixels += bytes((b, g, r, a))
        # AND mask: all zero (opacity comes from the alpha channel), rows padded to 4 bytes.
        mask_row = ((size + 31) // 32) * 4
        mask = bytes(mask_row * size)

        data = header + bytes(pixels) + mask
        payloads.append(data)
        entries.append(
            struct.pack(
                "<BBBBHHII",
                0 if size >= 256 else size,
                0 if size >= 256 else size,
                0,
                0,
                1,
                32,
                len(data),
                offset,
            )
        )
        offset += len(data)

    path.write_bytes(
        struct.pack("<HHH", 0, 1, len(images)) + b"".join(entries) + b"".join(payloads)
    )


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    cache: dict[int, bytes] = {}

    def get(size: int) -> bytes:
        if size not in cache:
            cache[size] = render(size)
        return cache[size]

    # The PNG set Tauri's bundler looks for.
    for size, name in [
        (32, "32x32.png"),
        (128, "128x128.png"),
        (256, "128x128@2x.png"),
        (512, "icon.png"),
    ]:
        write_png(OUT / name, get(size), size)
        print(f"  {name} ({size}x{size})")

    # Multi-resolution ICO for the Windows resource and the installer.
    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    write_ico(OUT / "icon.ico", [(s, get(s)) for s in ico_sizes])
    print(f"  icon.ico ({', '.join(str(s) for s in ico_sizes)})")


if __name__ == "__main__":
    main()
