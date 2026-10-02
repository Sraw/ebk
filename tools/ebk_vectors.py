#!/usr/bin/env python3
"""Synthetic test vectors for the ebk command: EPUBs the converter must refuse, and EBK files written
here from the specification (spec/ebk-format-1.0.md, sections 3-5 and 8-9) that the Rust reader must
accept or refuse. Shares no code with the Rust implementation.

Usage: .venv/bin/python tools/ebk_vectors.py <ebk_binary> <work_dir>
Exit status 0 when every case behaves as expected.
"""
import io, os, shutil, struct, subprocess, sys, time, warnings, zipfile, zlib

import brotli

MAGIC, END_MAGIC = b"\x89EBK\r\n\x1a\n", b"EBK\x1a"
CONTAINER = b'<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>'
OPF = b'<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/></spine></package>'
BASE = [("mimetype", b"application/epub+zip"), ("META-INF/container.xml", CONTAINER), ("OEBPS/c.opf", OPF), ("OEBPS/a.xhtml", b"<html><body><p>text</p></body></html>")]


def epub(path, extra=(), patch=None, fields={}):
    """fields: {name: bytes of the ZIP extra field of that entry}. The mimetype entry is stored, the others deflated."""
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")  # duplicate names are the point of one case
        with zipfile.ZipFile(path, "w") as z:
            for name, data in list(BASE) + list(extra):
                info = zipfile.ZipInfo(name)
                info.compress_type = zipfile.ZIP_STORED if name == "mimetype" else zipfile.ZIP_DEFLATED
                info.extra = fields.get(name, b"")
                z.writestr(info, data)
    if patch:
        blob = patch(open(path, "rb").read())
        open(path, "wb").write(blob)


def set_encrypted(blob):
    """Sets the 'encrypted' flag of the entry OEBPS/secret.txt in its local and central headers."""
    name = b"OEBPS/secret.txt"
    local = blob.index(b"PK\x03\x04", blob.index(name) - 30)
    central = blob.rindex(name) - 46
    assert blob[central:central + 4] == b"PK\x01\x02"
    out = bytearray(blob)
    out[local + 6] |= 1
    out[central + 8] |= 1
    return bytes(out)


def unicode_path(header_name, name):
    """The Info-ZIP Unicode Path extra field (0x7075): a second name for the entry."""
    body = b"\x01" + struct.pack("<I", zlib.crc32(header_name)) + name
    return struct.pack("<HH", 0x7075, len(body)) + body


def deflate(data):
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    return c.compress(data) + c.flush()


def zip_parts(entries, before=None, local=None, packed_as=None, base=0, stored=(), descriptor=None):
    """A ZIP file written field by field -> (local entries, central directory).
    before: {name: bytes put in front of that entry}; local: {name: fields of the local header that differ from the
    directory (method, crc, packed, lengths as (packed length, length))}; packed_as: {name: the entry's compressed
    bytes, in place of a proper deflate stream}; base: offset of the first entry; stored: names kept without
    compression; descriptor: {name: length of the data descriptor that follows the entry (12, 16, 20 or 24 bytes:
    with or without a signature, lengths of 32 or 64 bits), its local header left at zero}."""
    body, directory = b"", b""
    for name, data in entries:
        method = 0 if name in stored else 8
        packed, form, name = (packed_as or {}).get(name, deflate(data) if method else data), (descriptor or {}).get(name), name.encode()
        body += (before or {}).get(name.decode(), b"")
        flags = 8 if form else 0
        directory += struct.pack("<4sHHHHHHIIIHHHHHII", b"PK\x01\x02", 20, 20, flags, method, 0, 0x21, zlib.crc32(data), len(packed), len(data),
                                 len(name), 0, 0, 0, 0, 0, base + len(body)) + name
        other = (local or {}).get(name.decode(), {})
        in_file = other.get("packed", packed)
        sizes = other.get("lengths", (0, 0) if form else (len(in_file), len(data)))
        body += struct.pack("<4sHHHHHIIIHH", b"PK\x03\x04", 20, flags, other.get("method", method), 0, 0x21, other.get("crc", 0 if form else zlib.crc32(data)),
                            sizes[0], sizes[1], len(name), 0) + name + in_file
        if form:
            body += (b"PK\x07\x08" if form in (16, 24) else b"") + struct.pack("<IQQ" if form >= 20 else "<III", zlib.crc32(data), len(packed), len(data))
    return body, directory


def end_record(count, directory_len, directory_at, comment=b"", comment_len=None):
    return struct.pack("<4sHHHHIIH", b"PK\x05\x06", 0, 0, count, count, directory_len, directory_at, len(comment) if comment_len is None else comment_len) + comment


def raw_zip(entries=None, comment=b"", comment_len=None, after=b"", prefix=b"", **how):
    body, directory = zip_parts(entries or BASE, **how)
    return prefix + body + directory + end_record(len(entries or BASE), len(directory), len(body), comment, comment_len) + after


def two_directories():
    """The comment of the first end record holds a second directory and end record: two places end at the end of the file."""
    first, second = zip_parts(BASE)
    other = [(n, d.replace(b"text", b"OTHER")) for n, d in BASE]
    body2, directory2 = zip_parts(other, base=len(first))
    files = first + body2
    tail = directory2 + end_record(len(other), len(directory2), len(files) + len(second) + 22)
    return files + second + end_record(len(BASE), len(second), len(files), comment=tail)


def streamed_zip(damage=False):
    """Written by Python to a stream that cannot seek: every entry is followed by a data descriptor."""
    class NoSeek(io.RawIOBase):
        def __init__(self):
            self.data = bytearray()

        def writable(self):
            return True

        def write(self, d):
            self.data += d
            return len(d)
    out = NoSeek()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in BASE + [("OEBPS/dir/", b""), ("OEBPS/图/中文.xhtml", b"<p>x</p>")]:
            z.writestr(name, data)
    blob = bytes(out.data)
    if damage:  # the checksum in the first data descriptor
        at = blob.index(b"PK\x07\x08") + 4
        blob = blob[:at] + bytes([blob[at] ^ 1]) + blob[at + 1:]
    return blob


def second_occurrence(old, new):
    """Replaces the second place where `old` appears: after the name in the local header, that is its extra field."""
    def patch(blob):
        at = blob.index(old, blob.index(old) + 1)
        return blob[:at] + new + blob[at + len(old):]
    return patch


def end_record_counts(on_disk_delta, total_delta):
    """Changes the two entry counts of the end-of-central-directory record."""
    def patch(blob):
        at = blob.rindex(b"PK\x05\x06")
        on_disk, total = struct.unpack("<HH", blob[at + 8:at + 12])
        return blob[:at + 8] + struct.pack("<HH", on_disk + on_disk_delta, total + total_delta) + blob[at + 12:]
    return patch


def fake_end_record_in_comment(blob):
    """An archive comment that looks like an end record listing only the first four entries."""
    at = blob.rindex(b"PK\x05\x06")
    fake = b"PK\x05\x06" + struct.pack("<HHHHIIH", 0, 0, 4, 4, 0, 0, 0)
    return blob[:at + 20] + struct.pack("<H", len(fake)) + fake


def declare_length(name, length):
    """Sets the uncompressed length of an entry in its local header and in the central directory, leaving its data alone."""
    def patch(blob):
        local, central = blob.index(name) - 30, blob.rindex(name) - 46
        assert blob[local:local + 4] == b"PK\x03\x04" and blob[central:central + 4] == b"PK\x01\x02"
        blob = blob[:local + 22] + struct.pack("<I", length) + blob[local + 26:]
        return blob[:central + 24] + struct.pack("<I", length) + blob[central + 28:]
    return patch


HELPER = """
import os, resource, subprocess, sys
limit = int(os.environ.get("EBK_ADDRESS_SPACE_MB", "0")) << 20
r = subprocess.run(sys.argv[1:], preexec_fn=(lambda: resource.setrlimit(resource.RLIMIT_AS, (limit, limit))) if limit else None)
sys.stderr.write("\\nPEAK %d" % resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss)
sys.exit(r.returncode if r.returncode >= 0 else 100)
"""


def varint(v):
    out = bytearray()
    while v >= 0x80:
        out.append(v & 0x7F | 0x80)
        v >>= 7
    out.append(v)
    return bytes(out)


def ebk(blocks, members, resources=b"", version=(1, 0), reserved=b"\0" * 6, optional=b"", before_members=b"", quality=5):
    """blocks: [(raw_len, bytes)], members: [(mode, path, raw_len, stored_len, crc)]"""
    section = lambda tag, body: bytes([tag]) + varint(len(body)) + body
    index = section(1, varint(len(blocks)) + b"".join(varint(r) + varint(len(d)) for r, d in blocks))
    index += before_members
    index += section(3, varint(len(members)) + b"".join(
        bytes([mode]) + varint(len(p.encode())) + p.encode() + varint(raw) + varint(stored) + struct.pack("<I", crc) for mode, p, raw, stored, crc in members))
    index += optional
    packed = brotli.compress(index, quality=quality)
    return (MAGIC + bytes(version) + reserved + b"".join(d for _, d in blocks) + resources + packed
            + struct.pack("<III", len(packed), len(index), zlib.crc32(index)) + END_MAGIC)


def code(text, table):
    """Text coded with a character table (spec section 6, layout 1); written from the table of forms, not from any decoder."""
    out = bytearray()
    for c in text:
        cp, rank = ord(c), table.find(c)
        if cp < 0x80:
            out.append(cp)
        elif 0 <= rank < 64:
            out.append(0x80 + rank)
        elif rank >= 64:
            out += bytes([0xC0 + (rank - 64) // 128, 0x80 + (rank - 64) % 128])
        else:
            out += bytes([0xFF, 0x80 + (cp >> 14), 0x80 + (cp >> 7 & 0x7F), 0x80 + (cp & 0x7F)])
    return bytes(out)


def coded_book(table=None, charset_layout=1, mode=1, cut=None):
    """-> (file bytes, {path: bytes}): one plain member, two coded ones (70 characters in the table, so both one-byte
    and two-byte codes occur, and two characters that are not in it)."""
    chars = "".join(chr(0x4E00 + i) for i in range(70))
    text = "<p>" + chars * 40 + "é𠀀" + "</p>\n"
    texts = {"mimetype": b"application/epub+zip", "OEBPS/一.xhtml": text.encode(), "OEBPS/二.xhtml": ("二" + text[::-1]).encode()}
    table = chars if table is None else table
    stored = {p: (code(d.decode(), chars) if p != "mimetype" and mode == 1 else d) for p, d in texts.items()}
    stream = b"".join(stored.values())
    if cut:
        stream = stream[:cut] + bytes([stream[cut] ^ 0x40]) + stream[cut + 1:]
    members = [(mode if p != "mimetype" else 0, p, len(d), len(stored[p]), zlib.crc32(d)) for p, d in texts.items()]
    section = bytes([2]) + varint(1 + len(table.encode())) + bytes([charset_layout]) + table.encode()
    return ebk([(len(stream), brotli.compress(stream, quality=5))], members, before_members=section), texts


def resource_items(blob):
    """The members of an EBK file that are stored on their own -> [(mode, path, raw length, crc, stored bytes)]."""
    index_len = struct.unpack("<I", blob[-16:-12])[0]
    index = brotli.decompress(blob[-16 - index_len:-16])
    pos = 0

    def number():
        nonlocal pos
        v = shift = 0
        while True:
            b = index[pos]
            pos += 1
            v |= (b & 0x7F) << shift
            shift += 7
            if b < 0x80:
                return v
    at, items = 16, []
    while pos < len(index):
        tag = index[pos]
        pos += 1
        size = number()
        end = pos + size
        if tag == 1:
            for _ in range(number()):
                number()
                at += number()
        elif tag == 3:
            for _ in range(number()):
                mode = index[pos]
                pos += 1
                n = number()
                path = index[pos:pos + n].decode()
                pos += n
                raw_len, stored_len = number(), number()
                crc = struct.unpack("<I", index[pos:pos + 4])[0]
                pos += 4
                if mode >= 2:
                    items.append((mode, path, raw_len, crc, blob[at:at + stored_len]))
                    at += stored_len
        pos = end
    return items


def handoff_at(header):
    """Where the list of thread handoffs starts in an inflated Lepton header: after the JPEG's own headers."""
    return header.index(b"HH", 7 + struct.unpack("<I", header[3:7])[0])


def handoffs(header, rows):
    """An inflated Lepton header with its list of thread handoffs replaced by one entry for each first row given."""
    at = handoff_at(header)
    entry, after = header[at + 3:at + 19], header[at + 3 + 16 * header[at + 2]:]
    return header[:at] + b"HH" + bytes([len(rows)]) + b"".join(struct.pack("<H", row) + entry[2:] for row in rows) + after


def lepton_header(lepton, edit):
    """A Lepton file with its header (the part that holds the JPEG's own headers) changed by `edit`."""
    size = struct.unpack("<I", lepton[24:28])[0]
    packed = zlib.compress(edit(zlib.decompress(lepton[28:28 + size])))
    return lepton[:24] + struct.pack("<I", len(packed)) + packed + lepton[28 + size:]


def sample_book():
    """-> (file bytes, {path: bytes}); two blocks with a member across the boundary, a stored and a compressed resource."""
    texts = {"mimetype": b"application/epub+zip", "OEBPS/a.xhtml": "<p>第一章</p>".encode() * 3000, "OEBPS/empty.css": b"", "OEBPS/b.xhtml": b"<p>two</p>" * 5000}
    stream = b"".join(texts.values())
    cut = len(texts["mimetype"]) + 20000  # inside a.xhtml
    blocks = [(cut, brotli.compress(stream[:cut], quality=5)), (len(stream) - cut, brotli.compress(stream[cut:], quality=5))]
    png, font = os.urandom(300), bytes(range(256)) * 40
    packed_font = brotli.compress(font, quality=5)
    members = [(0, p, len(d), len(d), zlib.crc32(d)) for p, d in texts.items()]
    members.insert(2, (2, "OEBPS/图.png", len(png), len(png), zlib.crc32(png)))
    members.append((3, "OEBPS/f.ttf", len(font), len(packed_font), zlib.crc32(font)))
    optional = bytes([0x85]) + varint(5) + b"later"
    return ebk(blocks, members, png + packed_font, optional=optional), dict(texts, **{"OEBPS/图.png": png, "OEBPS/f.ttf": font})


def bomb():
    """About 50 KB that declare 32 GiB of text: 2048 blocks of 16 MiB of zeros, 64 members of 512 MiB."""
    block = brotli.compress(bytes(1 << 24), quality=5, lgwin=24)
    member = 512 << 20
    crc = 0
    for _ in range(member >> 24):
        crc = zlib.crc32(bytes(1 << 24), crc)
    return ebk([(1 << 24, block)] * 2048, [(0, f"m{n}", member, member, crc) for n in range(64)])


def main():
    binary, work = os.path.abspath(sys.argv[1]), sys.argv[2]
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    failures = []

    def run(*args, address_space_mb=0):
        """-> (exit status, output, peak memory in MiB, seconds); with address_space_mb the command gets no more memory than that"""
        # The command is started by a small helper, not by this process: on Linux a child's peak memory
        # begins at the size of the process it was started from, and this one holds the test data.
        start = time.time()
        r = subprocess.run([sys.executable, "-c", HELPER, binary, *args], capture_output=True, env=dict(os.environ, EBK_ADDRESS_SPACE_MB=str(address_space_mb)))
        text, _, tail = (r.stdout + r.stderr).decode("utf-8", "replace").rpartition("\nPEAK ")
        return r.returncode, text.strip(), int(tail) / 1024, time.time() - start

    def expect(name, ok, args, needle="", max_mb=None, max_s=None, address_space_mb=0):
        code, text, mb, seconds = run(*args, address_space_mb=address_space_mb)
        # exit status 1 is a reported error; anything else (a panic is 101, a signal is negative) is a crash
        good = code == (0 if ok else 1) and needle in text and "panicked" not in text
        good = good and (max_mb is None or mb <= max_mb) and (max_s is None or seconds <= max_s)
        limits = f"  [{mb:.0f} MiB, {seconds:.1f} s]" if max_mb or max_s else ""
        print(f"{'ok  ' if good else 'FAIL'}  {name}: exit {code}{limits}  {text.splitlines()[-1][:110] if text else ''}")
        if not good:
            failures.append(name)

    p = lambda name: os.path.join(work, name)

    # --- EPUBs
    epub(p("plain.epub"), [("OEBPS/", b""), ("OEBPS/img/", b"")])
    expect("epub with directory entries converts", True, ["convert", p("plain.epub"), "-o", p("plain.ebk")], "4 members")
    expect("and verifies against the epub", True, ["verify", p("plain.ebk"), "--epub", p("plain.epub")], "identical")
    # entries longer than the buffers a reader works with
    epub(p("large.epub"), [("OEBPS/long.xhtml", b"<p>" + bytes(range(32, 127)) * 9000 + b"</p>"), ("OEBPS/noise.bin", os.urandom(300000)), ("OEBPS/zeros.bin", bytes(5 << 20))])
    expect("epub with large entries converts", True, ["convert", p("large.epub"), "-o", p("large.ebk")], "7 members")
    expect("and verifies against the epub", True, ["verify", p("large.ebk"), "--epub", p("large.epub")], "identical")
    refused = {
        "duplicate name": dict(extra=[("OEBPS/a.xhtml", b"second")]),
        "name that is not UTF-8": dict(extra=[("OEBPS/XX.txt", b"x")], patch=lambda b: b.replace(b"OEBPS/XX.txt", b"OEBPS/\xff\xfe.txt")),
        "encrypted entry": dict(extra=[("OEBPS/secret.txt", b"x" * 100)], patch=set_encrypted),
        "backslash in a name": dict(extra=[("OEBPS/XX.txt", b"x")], patch=lambda b: b.replace(b"OEBPS/XX.txt", b"OEBPS\\XX.txt")),
        "parent directory in a name": dict(extra=[("OEBPS/XX/x.txt", b"x")], patch=lambda b: b.replace(b"OEBPS/XX/x.txt", b"OEBPS/../x.txt")),
        "empty segment in a name": dict(extra=[("OEBPS/X/x.txt", b"x")], patch=lambda b: b.replace(b"OEBPS/X/x.txt", b"OEBPS//Xx.txt")),
        "absolute name": dict(extra=[("XOEBPS/x.txt", b"x")], patch=lambda b: b.replace(b"XOEBPS/x.txt", b"/OEBPS/x.txt")),
        "control character in a name": dict(extra=[("OEBPS/X.txt", b"x")], patch=lambda b: b.replace(b"OEBPS/X.txt", b"OEBPS/\x07.txt")),
        "file that is also a directory": dict(extra=[("OEBPS/a.xhtml/x", b"x")]),
        # readers disagree about each of the following, so the converter takes none of them
        "duplicate name hidden by the total entry count": dict(extra=[("OEBPS/a.xhtml", b"second")], patch=end_record_counts(0, -1)),
        "duplicate name hidden by the entry count of this disk": dict(extra=[("OEBPS/a.xhtml", b"second")], patch=end_record_counts(-1, 0)),
        "second end record in the archive comment": dict(extra=[("OEBPS/a.xhtml", b"second")], patch=fake_end_record_in_comment),
        "file entry whose name ends in a backslash": dict(extra=[("OEBPS/secret\\", b"content")]),
        "directory entry with content": dict(extra=[("OEBPS/dirX", b"content")], patch=lambda b: b.replace(b"OEBPS/dirX", b"OEBPS/dir/")),
        "Unicode Path field with another name": dict(extra=[("OEBPS/b.xhtml", b"b")], fields={"OEBPS/b.xhtml": unicode_path(b"OEBPS/b.xhtml", b"OEBPS/other.xhtml")}),
        "Unicode Path field over a name that is not UTF-8": dict(
            extra=[("OEBPS/XXXX.txt", b"x")], patch=lambda b: b.replace(b"OEBPS/XXXX.txt", b"OEBPS/" + "中文".encode("gbk") + b".txt"),
            fields={"OEBPS/XXXX.txt": unicode_path(b"OEBPS/" + "中文".encode("gbk") + b".txt", "OEBPS/中文.txt".encode())}),
        "Unicode Path field with another name in the local header only": dict(
            extra=[("OEBPS/b.xhtml", b"b")], fields={"OEBPS/b.xhtml": unicode_path(b"OEBPS/b.xhtml", b"OEBPS/b.xhtml")}, patch=second_occurrence(b"OEBPS/b.xhtml", b"OEBPS/c.xhtml")),
        "entry longer than the directory declares": dict(extra=[("OEBPS/long.txt", b"x" * 5000)], patch=declare_length(b"OEBPS/long.txt", 4999)),
        "entry shorter than the directory declares": dict(extra=[("OEBPS/short.txt", b"x" * 5000)], patch=declare_length(b"OEBPS/short.txt", 1 << 30)),
    }
    for n, (name, how) in enumerate(refused.items()):
        epub(p(f"bad{n}.epub"), **how)
        expect(f"epub refused: {name}", False, ["convert", p(f"bad{n}.epub"), "-o", p(f"bad{n}.ebk")])
        if os.path.exists(p(f"bad{n}.ebk")):
            print("FAIL  an output file was left behind")
            failures.append(name + " (output left)")

    # --- ZIP files written field by field: what ZIP readers disagree about (second code review)
    xhtml = "OEBPS/a.xhtml"
    hidden = deflate(b"<html><body><p>seen by readers that go through the file</p></body></html>")
    orphan = struct.pack("<4sHHHHHIIIHH", b"PK\x03\x04", 20, 0, 8, 0, 0x21, 0, len(hidden), 73, len(xhtml), 0) + xhtml.encode() + hidden
    stored = deflate(BASE[3][1])
    deep = b'<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0">' + b"<a>" * 30000 + b"</a>" * 30000 + b"</package>"
    entities = (b'<?xml version="1.0"?><!DOCTYPE package [<!ENTITY a "' + b"<i/>" * 100 + b'">'
                + b"".join(b'<!ENTITY %c "%s">' % (c, (b"&%c;" % (c - 1)) * 100) for c in b"bcd") + b']><package xmlns="http://www.idpf.org/2007/opf" version="3.0">&d;</package>')
    for name, opf in {"elements nested 30000 deep": deep, "entities that expand to 100 million elements": entities}.items():
        open(p("opf.epub"), "wb").write(raw_zip(entries=BASE[:2] + [("OEBPS/c.opf", opf)] + BASE[3:]))
        expect(f"package document with {name}: converted in ZIP order", True, ["convert", p("opf.epub"), "-o", p("opf.ebk")], "4 members", max_mb=40, max_s=2)
    blob_bin = ("OEBPS/blob.bin", b"\xff" + os.urandom(999))
    forms = {form: raw_zip(entries=BASE + [blob_bin], descriptor={xhtml: form}) for form in (12, 16, 20, 24)}
    for form, blob in forms.items():
        open(p(f"descriptor{form}.epub"), "wb").write(blob)
        expect(f"data descriptor of {form} bytes converts", True, ["convert", p(f"descriptor{form}.epub"), "-o", p(f"descriptor{form}.ebk")], "5 members")
    hidden_entry = b"\xff" + b"PK\x07\x08" + os.urandom(200)
    raw_cases = {
        "stored entry with a data descriptor that holds the descriptor's signature": (raw_zip(entries=BASE + [("OEBPS/blob.bin", hidden_entry)], stored={"OEBPS/blob.bin"}, descriptor={"OEBPS/blob.bin": 16}), "descriptor's signature"),
        "data descriptor and a local header with another length": (raw_zip(entries=BASE + [blob_bin], descriptor={"OEBPS/blob.bin": 16}, local={"OEBPS/blob.bin": dict(lengths=(10, 10))}), "another length or checksum"),
        "directory entry named ..": (raw_zip(entries=BASE + [("../", b"")], stored={"../"}), "'..' segment"),
        "directory entry with compressed bytes that are no deflate stream": (raw_zip(entries=BASE + [("OEBPS/d/", b"")], packed_as={"OEBPS/d/": b"not deflate data"}), "directory entry with content"),
        "two places that could be the end record": (two_directories(), "more than one place"),
        "bytes after the end record": (raw_zip(after=b"trailing"), "not a ZIP file"),
        "archive comment shorter than declared": (raw_zip(comment_len=10), "not a ZIP file"),
        "bytes before the first entry": (raw_zip(prefix=b"#!/bin/sh\n"), "not where the end record says"),
        "local entry that the directory does not list": (raw_zip(before={xhtml: orphan}), "belong to no entry"),
        "other compression method in the local header": (raw_zip(local={xhtml: dict(method=0, packed=stored, crc=zlib.crc32(stored), lengths=(len(stored), len(stored)))}), "another compression method"),
        "other length in the local header": (raw_zip(local={xhtml: dict(lengths=(len(stored), 5))}), "another length or checksum"),
        "other checksum in the local header": (raw_zip(local={xhtml: dict(crc=1)}), "another length or checksum"),
        "damaged data descriptor": (streamed_zip(damage=True), "data descriptor"),
        "no META-INF/container.xml": (raw_zip(entries=BASE[:1] + BASE[2:]), "not an EPUB"),
        "no entries at all": (end_record(0, 0, 0), "not an EPUB"),
    }
    for n, (name, (blob, needle)) in enumerate(raw_cases.items()):
        open(p(f"raw{n}.epub"), "wb").write(blob)
        expect(f"epub refused: {name}", False, ["convert", p(f"raw{n}.epub"), "-o", p(f"raw{n}.ebk")], needle)
    unfinished = zlib.compressobj(9, zlib.DEFLATED, -15)
    streams = {
        "bytes after its deflate stream": (stored + b"hidden", "bytes after the end"),
        "a deflate stream without a last block": (unfinished.compress(BASE[3][1]) + unfinished.flush(zlib.Z_SYNC_FLUSH), "ends too early"),
    }
    for n, (name, (packed, needle)) in enumerate(streams.items()):
        open(p(f"stream{n}.epub"), "wb").write(raw_zip(packed_as={xhtml: packed}))
        expect(f"epub refused: {name}", False, ["convert", p(f"stream{n}.epub"), "-o", p(f"stream{n}.ebk")], needle)
    open(p("comment.epub"), "wb").write(raw_zip(comment=b"made by PK\x05\x06 tools"))
    expect("archive comment that holds the end record's signature converts", True, ["convert", p("comment.epub"), "-o", p("comment.ebk")], "4 members")
    open(p("streamed.epub"), "wb").write(streamed_zip())
    expect("ZIP with data descriptors converts", True, ["convert", p("streamed.epub"), "-o", p("streamed.ebk")], "5 members")
    expect("and verifies against the epub", True, ["verify", p("streamed.ebk"), "--epub", p("streamed.epub")], "identical")
    expect("verify --epub refuses an EPUB with a name twice", False, ["verify", p("plain.ebk"), "--epub", p("bad0.epub")], "two ZIP entries")

    leftovers = [f for f in os.listdir(work) if f.endswith(".tmp")]
    print(f"{'FAIL' if leftovers else 'ok  '}  no temporary file is left behind")
    failures.extend(leftovers)

    for name, data in {"sig4": b"PK\x05\x06", "sig8": b"PK\x05\x06\0\0\0\0", "empty": b"", "sig21": b"PK\x05\x06" + bytes(17)}.items():
        open(p(name + ".epub"), "wb").write(data)
        expect(f"epub refused: {len(data)} bytes starting like an end record", False, ["convert", p(name + ".epub"), "-o", p(name + ".ebk")], "not a ZIP file")

    epub(p("same-name.epub"), [("OEBPS/é.xhtml", b"x")], fields={"OEBPS/é.xhtml": unicode_path("OEBPS/é.xhtml".encode(), "OEBPS/é.xhtml".encode())})
    expect("Unicode Path field that repeats the name converts", True, ["convert", p("same-name.epub"), "-o", p("same-name.ebk")], "5 members")

    epub(p("many.epub"), [(f"OEBPS/f/{n:05}", b"") for n in range(65535 - len(BASE))])
    expect("65535 entries without ZIP64 convert", True, ["convert", p("many.epub"), "-o", p("many.ebk")], "65535 members")
    epub(p("zip64.epub"), [(f"OEBPS/f/{n:05}", b"") for n in range(65536 - len(BASE))])
    expect("65536 entries (ZIP64) convert", True, ["convert", p("zip64.epub"), "-o", p("zip64.ebk")], "65536 members")
    expect("and verify against the epub", True, ["verify", p("zip64.ebk"), "--epub", p("zip64.epub")], "identical")
    blob = open(p("zip64.epub"), "rb").read()
    at = blob.rindex(b"PK\x05\x06")
    open(p("zip64-disagree.epub"), "wb").write(blob[:at + 8] + struct.pack("<HH", 5, 5) + blob[at + 12:])
    expect("epub refused: ZIP64 end record and plain end record give other counts", False, ["convert", p("zip64-disagree.epub"), "-o", p("zip64-disagree.ebk")], "disagree")
    expect("a block size out of range is said to be that", False, ["convert", p("plain.epub"), "-o", p("never.ebk"), "--block-size", "1"], "--block-size")

    # 300 KiB that unpack to 300 MiB; .jpg so that the converter would store them as they are
    epub(p("zipbomb.epub"), [(f"OEBPS/z{n}.jpg", b"\xff" * (100 << 20)) for n in range(3)])
    print(f"      zipbomb.epub is {os.path.getsize(p('zipbomb.epub'))} bytes")
    expect("zip bomb: refused from the directory alone", False, ["convert", p("zipbomb.epub"), "-o", p("zipbomb.ebk"), "--max-input", str(200 << 20)], "--max-input", max_mb=20, max_s=1)
    expect("zip bomb: verify --epub has the same limit", False, ["verify", p("plain.ebk"), "--epub", p("zipbomb.epub"), "--max-output", str(200 << 20)], "unpacks to more than", max_mb=20)
    declared = max(n for n, name in enumerate(refused) if "shorter than the directory declares" in name)
    expect("a declared length of 1 GiB reserves nothing", False, ["convert", p(f"bad{declared}.epub"), "-o", p("declared.ebk")], "does not have the length", max_mb=20)

    shutil.copy(p("plain.epub"), p("inplace.ebk"))
    expect("convert does not replace its input", False, ["convert", p("inplace.ebk")], "replace the input")
    same = open(p("inplace.ebk"), "rb").read() == open(p("plain.epub"), "rb").read()
    print(f"{'ok  ' if same else 'FAIL'}  and the input is untouched")
    if not same:
        failures.append("input overwritten")

    epub(p("win.epub"), [("OEBPS/a:b.xhtml", b"x"), ("OEBPS/nul.txt", b"y")])
    expect("names unsafe on Windows convert", True, ["convert", p("win.epub"), "-o", p("win.ebk")])
    expect("but are not extracted", False, ["extract", p("win.ebk"), p("win-out")], "nothing was written")
    if os.path.exists(p("win-out")):
        print("FAIL  extract created the directory")
        failures.append("unsafe extract created files")

    # --- EBK files written from the specification
    blob, want = sample_book()
    open(p("spec.ebk"), "wb").write(blob)
    expect("file written from the spec verifies", True, ["verify", p("spec.ebk")], "6 members ok")
    expect("and extracts", True, ["extract", p("spec.ebk"), p("spec-out")])
    got = {os.path.relpath(os.path.join(d, f), p("spec-out")).replace(os.sep, "/"): open(os.path.join(d, f), "rb").read()
           for d, _, fs in os.walk(p("spec-out")) for f in fs}
    same = got == want
    print(f"{'ok  ' if same else 'FAIL'}  extracted members equal the originals ({len(got)} files)")
    if not same:
        failures.append("extract differs")
    expect("extract into a directory that is not empty", False, ["extract", p("spec.ebk"), p("spec-out")], "not empty")

    variants = {
        "major version 0 (the drafts)": blob[:8] + b"\x00\x03" + blob[10:],
        "major version 2": blob[:8] + b"\x02" + blob[9:],
        "reserved header bytes set": blob[:12] + b"\x01" + blob[13:],
        "flags set": blob[:10] + b"\x01" + blob[11:],
        "one byte appended": blob + b"\0",
        "last byte removed": blob[:-1],
        "one byte inserted after the header": blob[:16] + b"\0" + blob[16:],
    }
    for n, (name, data) in enumerate(variants.items()):
        open(p(f"v{n}.ebk"), "wb").write(data)
        expect(f"ebk refused: {name}", False, ["info", p(f"v{n}.ebk")], "not a valid EBK file")

    open(p("minor.ebk"), "wb").write(blob[:9] + b"\x07" + blob[10:])
    expect("a later minor version of major version 1 opens", True, ["verify", p("minor.ebk")], "6 members ok")
    damaged = bytearray(blob)
    damaged[40] ^= 0x10  # inside the first text block
    open(p("flip.ebk"), "wb").write(damaged)
    expect("damaged block: the file still opens", True, ["info", p("flip.ebk")])
    expect("damaged block: verify reports it", False, ["verify", p("flip.ebk")], "problems")

    # --- the code page (storage mode 1), written from section 6 of the specification
    blob, want = coded_book()
    open(p("coded.ebk"), "wb").write(blob)
    expect("coded members written from the spec verify", True, ["verify", p("coded.ebk")], "3 members ok")
    expect("info shows the code page", True, ["info", p("coded.ebk")], "")
    code_line = [line for line in run("info", p("coded.ebk"))[1].splitlines() if line.startswith("code page")]
    shown = code_line == ["code page        70 characters, used by 2 members"]
    print(f"{'ok  ' if shown else 'FAIL'}  {code_line}")
    if not shown:
        failures.append("info: code page line")
    expect("and extract", True, ["extract", p("coded.ebk"), p("coded-out")])
    got = {os.path.relpath(os.path.join(d, f), p("coded-out")).replace(os.sep, "/"): open(os.path.join(d, f), "rb").read()
           for d, _, fs in os.walk(p("coded-out")) for f in fs}
    print(f"{'ok  ' if got == want else 'FAIL'}  extracted coded members equal the originals ({len(got)} files)")
    if got != want:
        failures.append("coded extract differs")
    chars = "".join(chr(0x4E00 + i) for i in range(70))
    coded_cases = {
        "table in another order (other text of the same length)": (dict(table=chars[1] + chars[0] + chars[2:]), "verify", "2 problems"),
        "table one character short (a rank that is not there)": (dict(table=chars[:69]), "verify", "2 problems"),
        "a flipped bit in the coded text": (dict(cut=200), "verify", "problems"),
        "table of a layout from a later version": (dict(charset_layout=2), "verify", "2 problems"),
        "ASCII character in the table": (dict(table="a" + chars), "info", "not a valid EBK file"),
        "character twice in the table": (dict(table=chars + chars[0]), "info", "not a valid EBK file"),
        "table that no member uses": (dict(mode=0), "info", "not a valid EBK file"),
    }
    for n, (name, (how, command, needle)) in enumerate(coded_cases.items()):
        open(p(f"coded{n}.ebk"), "wb").write(coded_book(**how)[0])
        expect(f"code page: {name}", False, [command, p(f"coded{n}.ebk")], needle)
    expect("code page of a later layout: the file still opens", True, ["info", p("coded3.ebk")], "a layout this version does not know")
    expect("and is not extracted", False, ["extract", p("coded3.ebk"), p("coded3-out")], "nothing was written")

    state, chars = 1, []
    for _ in range(200000):  # some characters frequent, many rare, as in a book
        state = (state * 1103515245 + 12345) & 0x7FFFFFFF
        chars.append(chr(0x4E00 + (state >> 8) % (60 if state & 3 else 3000)))
    chapter = ("<html><body><p>" + "".join(chars) + "</p></body></html>").encode()
    epub(p("zh.epub"), [("OEBPS/b.xhtml", chapter), ("OEBPS/c.xhtml", chapter[::-1][:1000] + b"\xff"), ("OEBPS/d.xhtml", b"<p>plain</p>")])
    expect("chinese epub converts with a code page", True, ["convert", p("zh.epub"), "-o", p("zh.ebk")], "code page of")
    expect("and verifies against the epub", True, ["verify", p("zh.ebk"), "--epub", p("zh.epub")], "identical")
    expect("the same without a code page", True, ["convert", p("zh.epub"), "-o", p("zh-plain.ebk"), "--code-page", "never"], "text blocks")
    smaller = os.path.getsize(p("zh.ebk")) < os.path.getsize(p("zh-plain.ebk"))
    print(f"{'ok  ' if smaller else 'FAIL'}  the coded file is smaller ({os.path.getsize(p('zh.ebk'))} against {os.path.getsize(p('zh-plain.ebk'))})")
    if not smaller:
        failures.append("coded file is not smaller")
    expect("english epub converts without one", True, ["convert", p("plain.epub"), "-o", p("plain2.ebk")], "text blocks")
    plain = "code page" not in run("info", p("plain2.ebk"))[1]
    print(f"{'ok  ' if plain else 'FAIL'}  and has no code page")
    if not plain:
        failures.append("english book has a code page")

    # --- recompressed JPEG files (storage mode 4)
    data_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "ebk", "tests", "data")
    jpegs = [(f"OEBPS/{n}", open(os.path.join(data_dir, n), "rb").read()) for n in ("gray.jpg", "color.jpg", "progressive.jpg", "no-eoi.jpg", "trailing.jpg", "progressive-no-eoi.jpg")]
    epub(p("pictures.epub"), jpegs + [("OEBPS/not-a.jpg", os.urandom(2000)), ("OEBPS/renamed.png", jpegs[0][1][:-7] + b"another")])
    expect("epub with JPEG files converts", True, ["convert", p("pictures.epub"), "-o", p("pictures.ebk")], "12 members")
    expect("and verifies against the epub", True, ["verify", p("pictures.ebk"), "--epub", p("pictures.epub")], "identical")
    expect("the same with the JPEG files kept as they are", True, ["convert", p("pictures.epub"), "-o", p("pictures-kept.ebk"), "--keep-jpeg"], "12 members")
    items = resource_items(open(p("pictures.ebk"), "rb").read())
    modes = {path.split("/")[-1]: mode for mode, path, *_ in items}
    kept = [mode for mode, *_ in resource_items(open(p("pictures-kept.ebk"), "rb").read())]
    # the file without an end marker does not restore exactly and noise is no JPEG: those stay; a JPEG is known by its content
    as_expected = (modes["gray.jpg"], modes["color.jpg"], modes["trailing.jpg"], modes["no-eoi.jpg"], modes["renamed.png"], modes["not-a.jpg"]) == (4, 4, 4, 4, 4, 2) and modes["progressive-no-eoi.jpg"] in (2, 3) and 4 not in kept
    smaller = os.path.getsize(p("pictures.ebk")) < 0.9 * os.path.getsize(p("pictures-kept.ebk"))
    print(f"{'ok  ' if as_expected and smaller else 'FAIL'}  storage modes {modes}; {os.path.getsize(p('pictures.ebk'))} against {os.path.getsize(p('pictures-kept.ebk'))} bytes")
    if not (as_expected and smaller):
        failures.append("storage modes of JPEG files")
    line = [l for l in run("info", p("pictures.ebk"))[1].splitlines() if l.startswith("recompressed")]
    print(f"{'ok  ' if len(line) == 1 else 'FAIL'}  {line}")
    if len(line) != 1:
        failures.append("info: recompressed line")

    _, path, raw_len, crc, lepton = next(i for i in items if i[0] == 4 and i[1].endswith("color.jpg"))
    sof = lambda h: h.index(b"\xff\xc0")
    one = lambda payload, declared=raw_len: ebk([], [(4, "a.jpg", declared, len(payload), crc), (2, "ok", 1, 1, zlib.crc32(b"x"))], payload + b"x")
    # a Lepton file that says it is a JPEG four times its own length, in a member that says the same
    longer = lambda payload: one(payload[:20] + struct.pack("<I", 4 * len(payload)) + payload[24:], 4 * len(payload))
    hostile = {
        "the Lepton file as the converter wrote it": (one(lepton), True, "2 members ok"),
        "a flipped bit in the image data": (one(lepton[:-40] + bytes([lepton[-40] ^ 4]) + lepton[-39:]), False, "1 problems"),
        "an image of 16384 x 16384 in a file of 30 KB": (one(lepton_header(lepton, lambda h: h[:sof(h) + 5] + b"\x40\x00\x40\x00" + h[sof(h) + 9:])), False, "1 problems"),
        "an image of 8000 x 8000 (within the pixel limit, more than the data holds)": (one(lepton_header(lepton, lambda h: h[:sof(h) + 5] + b"\x1f\x40\x1f\x40" + h[sof(h) + 9:])), False, "1 problems"),
        "a header that declares 4 GiB": (one(lepton_header(lepton, lambda h: h[:3] + b"\xf0\xff\xff\xff" + h[7:])), False, "1 problems"),
        "garbage of 4 GiB declared after the image": (one(lepton_header(lepton, lambda h: h + b"GRB" + b"\xf0\xff\xff\xff")), False, "1 problems"),
        "a header without thread handoffs": (one(lepton_header(lepton, lambda h: h[:handoff_at(h)])), False, "1 problems"),
        "thread handoffs whose rows go backwards": (one(lepton_header(lepton, lambda h: handoffs(h, [8, 0]))), False, "1 problems"),
        "a thread handoff at an odd row of a subsampled image": (one(lepton_header(lepton, lambda h: handoffs(h, [0, 1]))), False, "1 problems"),
        "17 thread handoffs": (one(lepton_header(lepton, lambda h: handoffs(h, [0] * 17))), False, "1 problems"),
        "two lists of thread handoffs": (one(lepton_header(lepton, lambda h: h + handoffs(h, [0])[handoff_at(h):handoff_at(h) + 19])), False, "1 problems"),
        "another length in the Lepton header": (one(lepton[:20] + struct.pack("<I", raw_len + 1) + lepton[24:]), False, "1 problems"),
        "a Lepton file cut short": (one(lepton[:len(lepton) // 2]), False, "1 problems"),
        "bytes that are no Lepton file": (one(os.urandom(500), 600), False, "1 problems"),
        "an image of 2048 x 2048 in a file of 30 KB (more blocks than the file has bytes)": (one(lepton_header(lepton, lambda h: h[:sof(h) + 5] + b"\x08\x00\x08\x00" + h[sof(h) + 9:])), False, "1 problems"),
        "a header that inflates to 800 MiB": (longer(lepton_header(lepton, lambda h: h + b"GRB" + struct.pack("<I", 800 << 20) + bytes(800 << 20))), False, "1 problems"),
        "the same header record a million times": (longer(lepton_header(lepton, lambda h: h + (b"FRS" + struct.pack("<I", 200) + bytes(200)) * 1000000)), False, "1 problems"),
    }
    for n, (name, (blob, ok, needle)) in enumerate(hostile.items()):
        open(p(f"lepton{n}.ebk"), "wb").write(blob)
        expect(f"mode 4: {name}", ok, ["verify", p(f"lepton{n}.ebk")], needle, max_mb=120, max_s=5)
    open(p("lepton-long.ebk"), "wb").write(one(lepton, (1 << 27) + 1))
    expect("mode 4: a JPEG declared longer than 128 MiB", False, ["info", p("lepton-long.ebk")], "not a valid EBK file")
    open(p("lepton-ratio.ebk"), "wb").write(one(lepton[:100], 801))
    expect("mode 4: a JPEG declared more than eight times what is stored", False, ["info", p("lepton-ratio.ebk")], "not a valid EBK file")
    # a picture that deflate shrinks but that is no JPEG a codec would take: the converter falls back on brotli
    flat = bytes([0xFF, 0xD8, 0xFF, 0xE0]) + b"padding " * 4000
    epub(p("flat.epub"), [("OEBPS/flat.jpg", flat), ("OEBPS/zeros.png", b"\x89PNG" + bytes(50000))])
    expect("formats that are compressed already are compressed again where the ZIP did", True, ["convert", p("flat.epub"), "-o", p("flat.ebk")], "6 members")
    expect("and verify against the epub", True, ["verify", p("flat.ebk"), "--epub", p("flat.epub")], "identical")
    flat_modes = sorted(mode for mode, *_ in resource_items(open(p("flat.ebk"), "rb").read()))
    not_larger = os.path.getsize(p("flat.ebk")) < os.path.getsize(p("flat.epub"))
    print(f"{'ok  ' if flat_modes == [3, 3] and not_larger else 'FAIL'}  storage modes {flat_modes}, {os.path.getsize(p('flat.ebk'))} bytes against {os.path.getsize(p('flat.epub'))} for the EPUB")
    if not (flat_modes == [3, 3] and not_larger):
        failures.append("brotli where the ZIP compressed")

    # --- files from the code review of phase 1
    text = "<p>第一章</p>".encode() * 600
    for first, ok in ((4096, True), (4095, False)):
        blocks = [(first, brotli.compress(text[:first], quality=5)), (len(text) - first, brotli.compress(text[first:], quality=5))]
        open(p(f"short{first}.ebk"), "wb").write(ebk(blocks, [(0, "a", len(text), len(text), zlib.crc32(text))]))
        expect(f"first of two blocks has {first} bytes", ok, ["verify", p(f"short{first}.ebk")])
    one_byte = bytes([0x0F, 0x00, 0x80]) + b"A" + b"\x03"  # an uncompressed meta-block of one byte, then an empty last one
    assert brotli.decompress(one_byte) == b"A"
    open(p("tiny.ebk"), "wb").write(ebk([(1, one_byte)] * 100000, [(0, "one", 100000, 100000, zlib.crc32(b"A" * 100000))]))
    expect("ebk refused: 100000 blocks of one byte", False, ["info", p("tiny.ebk")], "not a valid EBK file", max_s=1)

    section = lambda tag, body: bytes([tag]) + varint(len(body)) + body
    open(p("charset.ebk"), "wb").write(ebk([], [], before_members=section(2, b"\x01" + "é".encode() * (30 << 20))))
    print(f"      charset.ebk is {os.path.getsize(p('charset.ebk'))} bytes with a character table of 60 MiB")
    expect("ebk refused: character table of 30 million characters", False, ["info", p("charset.ebk")], "not a valid EBK file", max_mb=100, max_s=2)

    zeros = bytes(1 << 24)
    block = brotli.compress(zeros, quality=5, lgwin=24)
    rest = (1 << 24) - 2000
    members = [(0, f"m{n}", 1, 1, zlib.crc32(b"\0")) for n in range(2000)] + [(0, "rest", rest, rest, zlib.crc32(bytes(rest)))]
    open(p("trailing.ebk"), "wb").write(ebk([(1 << 24, block + b"\0")], members))
    expect("a byte after a block's stream: every member fails, the block is decoded once", False, ["verify", p("trailing.ebk")], "2001 problems", max_s=3)
    open(p("trailing-ok.ebk"), "wb").write(ebk([(1 << 24, block)], members))
    expect("the same file without that byte", True, ["verify", p("trailing-ok.ebk")], "2001 members ok", max_s=3)

    # with too little memory for the 16 MiB block (or even for its decoder): an error for every member, at once, no abort
    for mb in (8, 20, 30):
        expect(f"the same file with {mb} MiB of address space: every member is too large, the block is tried once", False,
               ["verify", p("trailing-ok.ebk")], "2001 problems", max_s=3, address_space_mb=mb)
    expect("a file whose index does not fit in 8 MiB of address space is too large, not invalid", False, ["info", p("charset.ebk")], "too large", address_space_mb=8)

    open(p("members.ebk"), "wb").write(ebk([], [(0, "%05x" % n, 0, 0, 0) for n in range(1 << 20)], quality=9))
    print(f"      members.ebk is {os.path.getsize(p('members.ebk'))} bytes with 2^20 members")
    expect("2^20 members open in bounded memory", True, ["info", p("members.ebk")], "1048576", max_mb=100, max_s=3)

    # 64,000 paths of 1,024 bytes that share their first 1,016: the most work the member table can ask for per byte
    names = sorted((b"a" * 1016 + b"%08d" % n).decode() for n in range(64000))
    names = names[1::2] + names[0::2]
    open(p("prefix.ebk"), "wb").write(ebk([], [(0, name, 0, 0, 0) for name in names]))
    print(f"      prefix.ebk is {os.path.getsize(p('prefix.ebk'))} bytes with 64,000 paths of 1,024 bytes")
    expect("long paths with a long common prefix open in bounded time", True, ["info", p("prefix.ebk")], "64000", max_s=4)

    x = zlib.crc32(b"x")
    open(p("devices.ebk"), "wb").write(ebk([], [(2, "COM¹", 1, 1, x), (2, "a/CONIN$", 1, 1, x)], b"xx"))
    expect("device names with superscript digits are not extracted", False, ["extract", p("devices.ebk"), p("devices-out")], "nothing was written")
    open(p("long.ebk"), "wb").write(ebk([], [(2, "first", 1, 1, x), (2, "d/" + "n" * 256, 1, 1, x)], b"xx"))
    expect("a name of 256 bytes is not extracted", False, ["extract", p("long.ebk"), p("long-out")], "nothing was written")
    expect("extract checks the member limit first", False, ["extract", p("spec.ebk"), p("limit-out"), "--max-member", "1000"], "nothing was written")
    expect("verify reports members over the member limit", False, ["verify", p("spec.ebk"), "--max-member", "1000"], "problems")
    open(p("future.ebk"), "wb").write(ebk([], [(9, "future", 1 << 60, 0, 0), (2, "ok", 1, 1, x)], b"x"))
    expect("a member of a later mode with a huge length: verify goes on", False, ["verify", p("future.ebk")], "1 problems")
    expect("and extract writes nothing", False, ["extract", p("future.ebk"), p("future-out")], "nothing was written")
    open(p("escape.ebk"), "wb").write(ebk([], [(2, "a\u202eb\u009b", 1, 1, x)], b"x"))
    code, text, _, _ = run("info", "--members", p("escape.ebk"))
    escaped = code == 0 and "\u202e" not in text and "\u009b" not in text and "\\u{202e}" in text
    # the same in the message when a file cannot be created (a directory that may not be written to)
    os.makedirs(p("read-only"))
    os.chmod(p("read-only"), 0o500)
    code, text, _, _ = run("extract", p("escape.ebk"), p("read-only"))
    os.chmod(p("read-only"), 0o700)
    escaped = escaped and code == 1 and "\u202e" not in text and "\u009b" not in text and "\\u{202e}" in text
    print(f"{'ok  ' if escaped else 'FAIL'}  info prints bidi and C1 control characters escaped")
    if not escaped:
        failures.append("escaped paths")
    for d in ("devices-out", "future-out", "long-out", "limit-out", "coded3-out"):
        if os.path.exists(p(d)):
            print(f"FAIL  {d} was created")
            failures.append(d)

    open(p("bomb.ebk"), "wb").write(bomb())
    print(f"      bomb.ebk is {os.path.getsize(p('bomb.ebk'))} bytes and declares {64 * 512 >> 10} GiB")
    expect("bomb: info is instant", True, ["info", p("bomb.ebk")], "2048 blocks")
    expect("bomb: verify stops at the output limit", False, ["verify", p("bomb.ebk"), "--max-output", str(1 << 30)], "more than 1073741824 bytes")
    expect("bomb: extract stops at the output limit", False, ["extract", p("bomb.ebk"), p("bomb-out"), "--max-output", str(1 << 30)], "more than 1073741824 bytes")

    if os.path.exists(p("bomb-out")):
        print("FAIL  bomb-out was created")
        failures.append("bomb-out")
    print(f"\n{len(failures)} failures" + (": " + "; ".join(failures) if failures else ""))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
