#!/usr/bin/env python3
"""Seeds for the `jpeg`, `lepton`, `lepton_header` and `jpeg_encode` fuzz targets: small JPEG files of many kinds, and the Lepton file of each.

Usage: .venv/bin/python fuzz/jpeg_seeds.py     (needs Pillow; builds phase3/lepton-wasm for the encoder)
Kinds: grey and colour, three chroma subsamplings, baseline and progressive, with and without optimised Huffman
tables, restart markers, several qualities and sizes that are not multiples of the block size, files with
metadata, with bytes after the end marker, and cut short at several places.
"""
import io, math, os, struct, subprocess, tempfile, zlib

from PIL import Image

here = os.path.dirname(os.path.abspath(__file__))


def picture(mode, w, h, seed):
    img = Image.new(mode, (w, h))
    px, state = img.load(), seed * 2654435761 % (1 << 31) | 1
    for y in range(h):
        for x in range(w):
            state = (state * 1103515245 + 12345) & 0x7FFFFFFF
            v = int(127 + 90 * math.sin(x / 5 + seed) * math.cos(y / 7) + (state >> 20) % 30) % 256
            px[x, y] = v if mode == "L" else (v, (v * 2 + x * 3) % 256, (y * 5 + v // 2) % 256)
    return img


def unpack(lepton):
    """A Lepton file in the layout of the `lepton_header` target: the header inflated, with its length in front."""
    if len(lepton) < 32 or lepton[:2] != b"\xcf\x84":
        return None  # the seed of a JPEG file that the codec refuses is not a Lepton file
    packed_len = struct.unpack("<I", lepton[24:28])[0]
    header = zlib.decompress(lepton[28:28 + packed_len])
    if len(header) > 0xFFFF:
        return None
    return lepton[:28] + struct.pack("<H", len(header)) + header + lepton[28 + packed_len:-4]


def main():
    files, n = {}, 0
    for mode in ("L", "RGB"):
        for (w, h) in ((8, 8), (17, 9), (40, 33), (64, 64), (97, 61)):
            for how in (dict(), dict(progressive=True), dict(optimize=True), dict(restart_marker_blocks=3), dict(restart_marker_rows=1, optimize=True),
                        dict(quality=30), dict(quality=98), dict(subsampling=0), dict(subsampling=1), dict(subsampling=2, progressive=True)):
                if mode == "L" and "subsampling" in how:
                    continue
                out = io.BytesIO()
                picture(mode, w, h, n).save(out, "JPEG", **{"quality": 80, **how})
                name = f"{n:03}"
                files[name] = out.getvalue()
                n += 1
    base = files["055"]
    files["meta"] = base[:2] + b"\xff\xe1\x00\x10Exif\0\0" + bytes(8) + b"\xff\xfe\x00\x07hello" + base[2:]
    files["trailing"] = base + b"bytes after the image" * 3
    for cut in (2, 9, 40, len(base) // 2):
        files[f"cut{cut}"] = base[:-cut]
    with tempfile.TemporaryDirectory() as tmp:
        for name, data in files.items():
            open(os.path.join(tmp, name + ".jpg"), "wb").write(data)
        bench = os.path.join(here, "..", "phase3", "lepton-wasm")
        subprocess.run(["cargo", "build", "--quiet", "--release", "--bin", "seed"], cwd=bench, check=True)
        corpus = os.path.join(here, "corpus", "jpeg")
        os.makedirs(corpus, exist_ok=True)
        subprocess.run([os.path.join(bench, "target", "release", "seed"), corpus] + sorted(os.path.join(tmp, f) for f in os.listdir(tmp)), check=True)
        # the same Lepton files without the four bytes in front for the `lepton` target, the JPEG files for `jpeg_encode`
        for target in ("lepton", "jpeg_encode"):
            os.makedirs(os.path.join(here, "corpus", target), exist_ok=True)
        for f in os.listdir(corpus):
            if f.startswith("seed-"):
                open(os.path.join(here, "corpus", "lepton", f), "wb").write(open(os.path.join(corpus, f), "rb").read()[4:])
        for name, data in files.items():
            open(os.path.join(here, "corpus", "jpeg_encode", "seed-" + name), "wb").write(data)
        # and for `lepton_header`, the Lepton files in that target's layout
        os.makedirs(os.path.join(here, "corpus", "lepton_header"), exist_ok=True)
        for f in os.listdir(os.path.join(here, "corpus", "lepton")):
            if f.startswith("seed-"):
                unpacked = unpack(open(os.path.join(here, "corpus", "lepton", f), "rb").read())
                if unpacked:
                    open(os.path.join(here, "corpus", "lepton_header", f), "wb").write(unpacked)


if __name__ == "__main__":
    main()
