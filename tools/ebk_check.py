#!/usr/bin/env python3
"""Convert every EPUB in the given directories with the ebk command and check the results with an
independent reader: this file parses .ebk from the specification and shares no code with the Rust
implementation. It is a checker of what the converter writes, not a general reader: files that use
storage modes or character-table layouts of a later version, which a reader must open, are refused here.

Usage: .venv/bin/python tools/ebk_check.py <ebk_binary> <out_dir> <out.json> <corpus_dir>...
EVAL_WORKERS sets the number of conversions at a time; EBK_CONVERT_ARGS adds arguments to `ebk convert`.

Per book: every member of the EPUB must come out of the .ebk byte for byte (compared by sha256), and
no other member may exist. Sizes are reported as in phase 0: "text view" = the .ebk file without the
stored bytes of image members, against the EPUB's ZIP records of the same members.
"""
import hashlib, json, os, re, struct, subprocess, sys, tempfile, time, zipfile, zlib
from concurrent.futures import ThreadPoolExecutor

import brotli

MAGIC, END_MAGIC = b"\x89EBK\r\n\x1a\n", b"EBK\x1a"


def is_image(data):
    """JPEG, PNG, GIF or WebP, by their first bytes (exporters do not always keep the extensions)."""
    return data[:3] == b"\xff\xd8\xff" or data[:8] == b"\x89PNG\r\n\x1a\n" or data[:6] in (b"GIF87a", b"GIF89a") or (data[:4] == b"RIFF" and data[8:12] == b"WEBP")
# Storage mode 4 is decoded by lepton_jpeg as published on crates.io (tools/lepton-check), not by the ebk crate.
UNLEPTON = os.path.join(os.path.dirname(os.path.abspath(__file__)), "lepton-check", "target", "release", "unlepton")


def unlepton(payloads):
    """{path: Lepton bytes} -> {path: JPEG bytes}"""
    with tempfile.TemporaryDirectory() as tmp:
        names = {}
        for n, (path, data) in enumerate(payloads.items()):
            names[path] = os.path.join(tmp, f"{n}.lep")
            open(names[path], "wb").write(data)
        files = list(names.values())
        for start in range(0, len(files), 500):
            run = subprocess.run([UNLEPTON] + files[start:start + 500], capture_output=True, text=True)
            assert run.returncode == 0, f"Lepton data does not decode: {run.stdout[-200:]} {run.stderr[-200:]}"
        return {path: open(name + ".jpg", "rb").read() for path, name in names.items()}


def check_lepton(path, lepton, jpeg):
    """The reader rules of spec section 7 that the upstream decoder does not have: the length in the Lepton header,
    the length of its inflated header, the pixel limit, and one byte of JPEG for every 8x8 block."""
    assert len(lepton) >= 28 and struct.unpack("<I", lepton[20:24])[0] == len(jpeg), f"{path}: length in the Lepton header"
    header = zlib.decompressobj().decompress(lepton[28:28 + struct.unpack("<I", lepton[24:28])[0]], len(jpeg) + 65536 + 1)
    assert len(header) <= len(jpeg) + 65536, f"{path}: Lepton header longer than the JPEG"
    pos = 2
    while pos + 9 < len(jpeg) and jpeg[pos] == 0xFF:
        marker, length = jpeg[pos + 1], struct.unpack(">H", jpeg[pos + 2:pos + 4])[0]
        if 0xC0 <= marker <= 0xCF and marker not in (0xC4, 0xC8, 0xCC):
            height, width = struct.unpack(">HH", jpeg[pos + 5:pos + 9])
            assert width * height <= 1 << 26 and width * height // 64 <= len(jpeg), f"{path}: {width}x{height} pixels in {len(jpeg)} bytes"
            return
        pos += 2 + length
    raise AssertionError(f"{path}: no frame header in the restored JPEG")
EXTRA_ARGS = os.environ.get("EBK_CONVERT_ARGS", "").split()  # e.g. "--block-size 1048576"


class Cursor:
    def __init__(self, buf):
        self.buf, self.pos = buf, 0

    def take(self, n):
        assert self.pos + n <= len(self.buf), "field runs past the end"
        self.pos += n
        return self.buf[self.pos - n:self.pos]

    def varint(self):
        v = shift = 0
        while True:
            b = self.take(1)[0]
            v |= (b & 0x7F) << shift
            shift += 7
            if b < 0x80:
                assert b or shift == 7, "varint not in shortest form"
                assert v < 1 << 63
                return v

    def done(self):
        return self.pos == len(self.buf)


def packed_bound(n):
    return n + n // 4096 + 64


def check_paths(paths):
    for p in paths:
        assert 1 <= len(p.encode()) <= 1024 and not any(ord(c) < 0x20 or c in "\x7f\\" for c in p), f"path {p!r}"
        assert all(seg not in ("", ".", "..") for seg in p.split("/")), f"path segments {p!r}"
    # sorted with "/" before every other character, a path is directly followed by the paths under it
    ordered = sorted(paths, key=lambda p: p.encode().replace(b"/", b"\0"))
    for a, b in zip(ordered, ordered[1:]):
        assert a != b, "duplicate path"
        assert not b.startswith(a + "/"), f"{b!r} lies under another member"


CODED = re.compile(rb"[\x00-\x7f]+|[\x80-\xbf]|[\xc0-\xfe][\x80-\xff]|\xff[\x80-\xff]{3}")


def read_charset(section):
    """-> decoder for members stored with the code page (spec section 6, layout 1)"""
    assert section.take(1) == b"\x01", "character table layout"
    chars = section.take(len(section.buf) - 1).decode("utf-8")
    assert len(chars) <= 8128 and len(set(chars)) == len(chars) and all(c > "\x7f" for c in chars), "character table"
    code = {}
    for rank, c in enumerate(chars):
        key = bytes([0x80 + rank]) if rank < 64 else bytes([0xC0 + (rank - 64) // 128, 0x80 + (rank - 64) % 128])
        code[key] = c.encode("utf-8")
    listed = set(chars)

    def decode(stored):
        used = 0

        def one(m):
            nonlocal used
            b = m.group()
            used += len(b)
            if b[0] < 0x80:
                return b
            if b[0] < 0xFF:
                assert b in code, "rank beyond the character table"
                return code[b]
            cp = (b[1] - 0x80) << 14 | (b[2] - 0x80) << 7 | (b[3] - 0x80)
            assert 0x7F < cp <= 0x10FFFF and not 0xD800 <= cp <= 0xDFFF and chr(cp) not in listed, "four-byte form"
            return chr(cp).encode("utf-8")

        text = CODED.sub(one, stored)
        assert used == len(stored), "coded text is cut off or has a second byte below 0x80"
        return text

    return decode


def read_ebk(blob):
    """-> ({path: original bytes}, {path: (mode, stored_len)}, compressed index length, [block raw lengths])"""
    assert len(blob) >= 32 and blob[:8] == MAGIC and blob[8] == 1 and blob[10:16] == b"\0" * 6, "header"
    index_len, index_raw_len, index_crc, end = struct.unpack("<III4s", blob[-16:])
    assert end == END_MAGIC, "end signature"
    assert 1 <= index_len <= min(64 << 20, len(blob) - 32, packed_bound(index_raw_len)) and index_raw_len <= 64 << 20, "index length"
    index = brotli.decompress(blob[len(blob) - 16 - index_len:len(blob) - 16])
    assert len(index) == index_raw_len and zlib.crc32(index) == index_crc, "index length or checksum"
    sections, cur, last = {}, Cursor(index), 0
    while not cur.done():
        tag = cur.take(1)[0]
        assert tag > last, "section order"
        last = tag
        sections[tag] = Cursor(cur.take(cur.varint()))
    assert {1, 3} <= {t for t in sections if t < 0x80} <= {1, 2, 3}, f"sections {sorted(sections)}"  # 0x80 and up are optional

    c = sections[1]
    blocks = [(c.varint(), c.varint()) for _ in range(c.varint())]
    assert c.done() and len(blocks) <= 1 << 20 and all(1 <= r <= 1 << 24 and 1 <= p <= packed_bound(r) for r, p in blocks), "block table"
    assert all(r >= 4096 for r, _ in blocks[:-1]), "a block that is not the last is shorter than 4096 bytes"
    c = sections[3]
    members = []
    for _ in range(c.varint()):
        mode = c.take(1)[0]
        path = c.take(c.varint()).decode("utf-8")
        raw_len, stored_len, crc = c.varint(), c.varint(), struct.unpack("<I", c.take(4))[0]
        members.append((mode, path, raw_len, stored_len, crc))
    assert c.done() and len(members) <= 1 << 20
    check_paths([m[1] for m in members])
    assert (2 in sections) == any(m[0] == 1 for m in members), "character table without coded members, or the reverse"
    decode = read_charset(sections[2]) if 2 in sections else None

    pos, stream = 16, b""
    assert sum(r for r, _ in blocks) == sum(m[3] for m in members if m[0] < 2), "blocks do not add up to the text stream"
    for raw_len, packed_len in blocks:
        block = brotli.decompress(blob[pos:pos + packed_len])
        assert len(block) == raw_len, "block length"
        stream += block
        pos += packed_len
    out, stored, at, lepton = {}, {}, 0, {}
    for mode, path, raw_len, stored_len, crc in members:
        if mode == 0:
            assert stored_len == raw_len
            data = stream[at:at + stored_len]
            at += stored_len
        elif mode == 1:
            assert 1 <= raw_len <= 4 * stored_len and stored_len <= 2 * raw_len, f"{path}: lengths"
            data = decode(stream[at:at + stored_len])
            at += stored_len
        else:
            assert mode in (2, 3, 4), f"mode {mode}"
            assert stored_len == raw_len if mode == 2 else 1 <= stored_len < raw_len, f"{path}: lengths"
            assert mode != 4 or (raw_len <= 1 << 27 and raw_len <= 8 * stored_len), f"{path}: lengths of a recompressed JPEG"
            data = blob[pos:pos + stored_len]
            pos += stored_len
            if mode == 3:
                data = brotli.decompress(data)
            elif mode == 4:
                lepton[path] = data
        out[path], stored[path] = data, (mode, stored_len, raw_len, crc)
    assert pos + index_len + 16 == len(blob), "bytes not described by the index"
    out.update(unlepton(lepton) if lepton else {})
    for path, data in lepton.items():
        check_lepton(path, data, out[path])
    for path, (mode, stored_len, raw_len, crc) in stored.items():
        assert len(out[path]) == raw_len and zlib.crc32(out[path]) == crc, f"{path}: length or checksum"
        stored[path] = (mode, stored_len)
    return out, stored, index_len, [r for r, _ in blocks]


def check(binary, out_dir, epub):
    book = os.path.basename(epub)
    ebk = os.path.join(out_dir, book[:-5] + ".ebk")
    t0 = time.time()
    run = subprocess.run([binary, "convert", epub, "-o", ebk, "--threads", "2"] + EXTRA_ARGS, capture_output=True, text=True)
    if run.returncode:
        return {"book": book, "error": "convert: " + run.stderr.strip()[-300:]}
    seconds = time.time() - t0
    try:
        blob = open(ebk, "rb").read()
        got, stored, index_len, blocks = read_ebk(blob)
        want = {}
        with zipfile.ZipFile(epub) as z:
            for i in z.infolist():
                if not i.is_dir():
                    name = i.filename if i.flag_bits & 0x800 else i.filename.encode("cp437").decode("utf-8")
                    want[name] = z.read(i)
        assert set(got) == set(want), f"member names differ: {sorted(set(got) ^ set(want))[:3]}"
        bad = [n for n in want if hashlib.sha256(got[n]).digest() != hashlib.sha256(want[n]).digest()]
        assert not bad, f"members differ: {bad[:3]}"
        images = sum(stored[n][1] for n, d in want.items() if is_image(d))
        modes = [m for m, _ in stored.values()]
        return {"book": book, "epub": os.path.getsize(epub), "ebk": len(blob), "text_view": len(blob) - images, "index": index_len, "blocks": blocks,
                "members": len(want), "in_stream": modes.count(0) + modes.count(1), "coded": modes.count(1), "brotli_resources": modes.count(3), "jpeg": modes.count(4),
                "jpeg_stored": sum(s for m, s in stored.values() if m == 4), "jpeg_raw": sum(len(want[n]) for n, (m, _) in stored.items() if m == 4),
                "seconds": round(seconds, 2)}
    except Exception as e:
        return {"book": book, "error": f"check: {type(e).__name__}: {e}"[:300]}


def main():
    binary, out_dir, out_json, dirs = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:]
    os.makedirs(out_dir, exist_ok=True)
    epubs = sorted((os.path.join(d, f) for d in dirs for f in os.listdir(d) if f.endswith(".epub")), key=os.path.getsize, reverse=True)
    t0 = time.time()
    with ThreadPoolExecutor(int(os.environ.get("EVAL_WORKERS", "6"))) as pool:  # threads: the work is in subprocesses and C extensions
        rows = list(pool.map(lambda p: check(binary, out_dir, p), epubs))
    errors = [r for r in rows if "error" in r]
    good = sorted((r for r in rows if "error" not in r), key=lambda r: r["book"])
    json.dump({"books": len(good), "errors": errors, "elapsed_s": round(time.time() - t0), "per_book": good}, open(out_json, "w"), indent=1)
    print("books", len(good), "errors", len(errors), "elapsed", round(time.time() - t0), "s")
    for e in errors[:20]:
        print("  ", e["book"], e["error"])
    total = lambda key: sum(r[key] for r in good)
    print(f"epub {total('epub')}  ebk {total('ebk')}  ratio {total('ebk') / max(1, total('epub')):.4f}  index {total('index')}")
    return 1 if errors or not good else 0


if __name__ == "__main__":
    sys.exit(main())
