#!/usr/bin/env python3
"""Seed corpora for the fuzz targets, from the files that tools/ebk_vectors.py leaves in its work directory.

Usage: .venv/bin/python fuzz/seeds.py <vector work dir>
raw: the .ebk files as they are; index: data area + uncompressed index (see src/lib.rs); epub: the .epub files;
jpeg: the Lepton files of the mode-4 vectors.
Files over 64 KiB are left out (the targets run with -max_len=65536).
"""
import os, struct, sys

import brotli

here = os.path.dirname(os.path.abspath(__file__))
work = sys.argv[1]
counts = {}
for name in sorted(os.listdir(work)):
    path = os.path.join(work, name)
    if not os.path.isfile(path) or os.path.getsize(path) > 65536:
        continue
    stem, ext = os.path.splitext(name)
    blob = open(path, "rb").read()
    outputs = {}
    if ext == ".epub":
        outputs["epub"] = blob
    elif ext == ".ebk":
        outputs["raw"] = blob
        if len(blob) >= 32:
            packed_len = struct.unpack("<I", blob[-16:-12])[0]
            body = blob[16:len(blob) - 16 - packed_len]
            try:
                index = brotli.decompress(blob[len(blob) - 16 - packed_len:-16])
                outputs["index"] = struct.pack("<H", len(body)) + body + index
            except (brotli.error, struct.error):
                pass
    if ext == ".ebk" and stem.startswith("lepton") and "index" in outputs:
        # one recompressed JPEG and a one-byte member after it: the input of the jpeg target is the declared length and the Lepton file
        body = outputs["index"][2:2 + struct.unpack("<H", outputs["index"][:2])[0]]
        length = struct.unpack("<I", body[20:24])[0] if len(body) > 24 else 0
        outputs["jpeg"] = struct.pack("<I", length) + body[:-1]
    for target, data in outputs.items():
        os.makedirs(os.path.join(here, "corpus", target), exist_ok=True)
        open(os.path.join(here, "corpus", target, "seed-" + stem), "wb").write(data)
        counts[target] = counts.get(target, 0) + 1
print(counts)
