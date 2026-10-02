#!/usr/bin/env python3
"""Makes the JPEG files of this directory (needs Pillow). The pictures are generated, not taken from anywhere.

gray.jpg, color.jpg     baseline, one and three components (4:2:0)
progressive.jpg         progressive, as libjpeg writes it: the Lepton encoder refuses these
no-eoi.jpg              color.jpg without its last two bytes: Lepton encodes it but restores other bytes
trailing.jpg            color.jpg followed by bytes that are not part of the image
"""
import io, math, os

from PIL import Image

here = os.path.dirname(os.path.abspath(__file__))


def picture(mode):
    w, h = 320, 240
    img = Image.new(mode, (w, h))
    px = img.load()
    state = 7
    for y in range(h):
        for x in range(w):
            state = (state * 1103515245 + 12345) & 0x7FFFFFFF
            v = int(127 + 90 * math.sin(x / 17) * math.cos(y / 23) + (state >> 20) % 24)
            px[x, y] = v if mode == "L" else (v, (v * 2 + x) % 256, (y + v // 2) % 256)
    return img


def jpeg(img, **how):
    out = io.BytesIO()
    img.save(out, "JPEG", quality=80, **how)
    return out.getvalue()


color, progressive = jpeg(picture("RGB")), jpeg(picture("RGB"), progressive=True)
files = {
    "gray.jpg": jpeg(picture("L")),
    "color.jpg": color,
    "progressive.jpg": progressive,
    "no-eoi.jpg": color[:-2],
    "progressive-no-eoi.jpg": progressive[:-2],
    "trailing.jpg": color + b"bytes after the end of the image",
}
for name, data in files.items():
    open(os.path.join(here, name), "wb").write(data)
    print(name, len(data))
