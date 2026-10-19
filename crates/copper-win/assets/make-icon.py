#!/usr/bin/env python3
"""Draw Copper's Windows icon (copper.ico) from the Mac app's copper plate.

The Mac icon (copper-mac-ref/Icon/icon.swift) is plate.jpg — a macro of
weathered copper with a river of azurite and malachite across it — clipped to
a rounded square. Windows wants the same plate, but on its own grid: Windows
11 icons fill more of their square than macOS's 824/1024, so the plate here
takes 15/16 of the canvas at large sizes and nearly all of it at the small
ones, where every pixel of copper counts. Each size is drawn on its own (mask
supersampled 8x, plate resampled with Lanczos), not shrunk from the 256.

Usage: python3 make-icon.py <path/to/plate.jpg> [out.ico]
Needs Pillow. Run once by hand; the .ico is checked in and build.rs embeds it.
"""

import io
import struct
import sys

from PIL import Image, ImageDraw

SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
SUPERSAMPLE = 8


def margin_for(size: int) -> float:
    """Air around the plate, in pixels: half a pixel when tiny, 1/32 above."""
    if size <= 24:
        return 0.5
    return size / 32


def radius_for(plate: float) -> float:
    """Corner radius: the Mac's 22.37 % of the plate's side."""
    return plate * 0.2237


def draw(plate_img: Image.Image, size: int) -> Image.Image:
    m = margin_for(size)
    side = size - 2 * m
    big = size * SUPERSAMPLE
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (m * SUPERSAMPLE, m * SUPERSAMPLE, (size - m) * SUPERSAMPLE - 1, (size - m) * SUPERSAMPLE - 1),
        radius=radius_for(side) * SUPERSAMPLE,
        fill=255,
    )
    mask = mask.resize((size, size), Image.Resampling.LANCZOS)
    face = plate_img.resize((size, size), Image.Resampling.LANCZOS)
    out = face.convert("RGBA")
    out.putalpha(mask)
    return out


def ico(images: list[Image.Image]) -> bytes:
    """An ICO with a PNG-compressed 256 and 32-bpp BMP entries for the rest
    (the shell's small-icon paths still expect BMP at the small sizes)."""
    blobs = []
    for im in images:
        w = im.width
        if w >= 256:
            # A 256-colour palette with error diffusion is indistinguishable
            # from truecolour on a photo this busy, at a quarter of the bytes
            # — and every byte here is a byte of copper.exe.
            quantized = im.quantize(256, method=Image.Quantize.FASTOCTREE, dither=Image.Dither.FLOYDSTEINBERG)
            buf = io.BytesIO()
            quantized.save(buf, format="PNG", optimize=True)
            blobs.append(buf.getvalue())
            continue
        # BITMAPINFOHEADER + bottom-up BGRA rows + 1-bpp AND mask (all zero:
        # the alpha channel carries the shape).
        header = struct.pack("<IiiHHIIiiII", 40, w, w * 2, 1, 32, 0, 0, 0, 0, 0, 0)
        px = im.tobytes("raw", "BGRA")
        rows = [px[y * w * 4:(y + 1) * w * 4] for y in range(w)]
        xor = b"".join(reversed(rows))
        and_stride = ((w + 31) // 32) * 4
        blobs.append(header + xor + b"\0" * and_stride * w)
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    for im, blob in zip(images, blobs):
        dim = 0 if im.width >= 256 else im.width
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    return head + entries + b"".join(blobs)


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    plate = Image.open(sys.argv[1]).convert("RGB")
    out = sys.argv[2] if len(sys.argv) > 2 else "copper.ico"
    with open(out, "wb") as f:
        f.write(ico([draw(plate, s) for s in SIZES]))
    print(f"wrote {out}: {', '.join(map(str, SIZES))}")


if __name__ == "__main__":
    main()
