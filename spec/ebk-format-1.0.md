# The EBK format, version 1.0

Status: release candidate, 2026-10-02. This is the English text of the specification; `ebk-format-1.0.zh.md` is the
same specification in Chinese, in which it was drafted. Where the two differ, this text governs.
Reference implementation: `crates/ebk` (reader and writer), `crates/ebk-cli` (converter), `crates/ebk-wasm` (reader for the web).
Licence of this document: CC BY 4.0.

The words "must", "must not", "should" and "may" are used as MUST, MUST NOT, SHOULD and MAY in RFC 2119.
Passages marked "(informative)" are not requirements.

## 1. Overview

EBK is a read-only container for e-books, produced by converting an EPUB. It keeps the path and the original
bytes of every file of the EPUB (a **member**). A reading system takes members out by path and hands them to an
existing EPUB renderer. EBK defines no content model of its own, and it does not promise to give back the EPUB
file it was made from: the structure of the ZIP file, the order of its entries and their time stamps are not kept.

Design goals:

- Text members are joined and compressed in blocks, so that the file is nearly as small as the book compressed as
  one stream, while opening any chapter needs only a few blocks.
- The bytes read for a member are exactly the bytes it had in the EPUB (every member has a checksum).
- Given a damaged or hostile file, a reader returns the right bytes or an error; it must not crash, read out of
  bounds, or use memory or time without limit.

The format **does not protect against tampering**: the checksums find accidental damage and implementation
errors, and anyone can change the content and compute them again.

File name extension `.ebk`; media type `application/x-ebk` until one is registered.

## 2. Conventions

- Multi-byte integers are little-endian. `u8`, `u16`, `u32` are unsigned integers of fixed width.
- A **varint** is an unsigned LEB128 number: each byte carries seven bits, low group first, and its top bit says
  that another byte follows. Values do not exceed 2^63 − 1, so a varint has at most 9 bytes. It must be in its
  shortest form: the last byte of a varint of more than one byte must not be 0x00. A reader must reject a varint
  whose ninth byte has the top bit set, and one that is not in its shortest form.
- **CRC-32** is the one used by ZIP and PNG (polynomial 0xEDB88320, initial value and final XOR 0xFFFFFFFF).
  (Informative: the ZIP directory of an EPUB already holds this checksum for every member, so a converter can
  compare directly.)
- **Valid UTF-8** is as defined by RFC 3629 (no surrogate code points, no overlong forms, nothing above U+10FFFF).
- All arithmetic on lengths and offsets (sums, and products such as `4 × stored length`) must be done in unsigned
  64-bit numbers with overflow checked, and nothing may be narrowed to a smaller type before it has passed its
  range check.
- An **exact brotli stream**: "c bytes are an exact brotli stream of n bytes" means all of
  1. they are a stream as defined by RFC 7932, whose header declares a window of at most 2^24 (no large-window
     extension), with no shared dictionary;
  2. the stream ends exactly at byte c (no input is left over);
  3. it decompresses to exactly n bytes.

  A reader must check all three and must not produce more than n bytes of output while doing so.
- **Bound on compressed length**: the compressed length c of a text block or of the index must also satisfy
  `1 ≤ c ≤ n + n div 4096 + 64`. The bound keeps a stream from carrying large amounts of data that produce no
  output. The brotli format can always meet it (data that does not compress can be stored as it is), but encoders
  do not promise to: the Rust `brotli` encoder exceeds it on random data at qualities 0 and 1. A writer must check
  its output and compress again with other settings when the bound is exceeded.

## 3. File structure

```
+------------------+  offset 0
| header, 16 bytes |
+------------------+  offset 16
| text blocks      |  blocks 0..B-1, one after the other
+------------------+
| resource area    |  resource items, one after the other
+------------------+
| index            |  one brotli stream
+------------------+
| footer, 16 bytes |
+------------------+  end of file
```

There is no padding between the parts and there are no offset fields: the text blocks start at offset 16 and
their total length is the sum of the compressed lengths of the blocks; the resource area follows, and its length
is the sum of the stored lengths of the resource items; the footer says where the index is. A reader must check that

```
16 + length of the text blocks + length of the resource area + compressed length of the index + 16 == file length
```

and reject the file otherwise. Every byte of the file then belongs to exactly one region that the index describes.
(This does not mean that the content of every byte has been checked: members stored in a way the reader does not
know, and optional index sections, are opaque to it.)

### 3.1 Header (16 bytes)

| Offset | Type | Content |
|---|---|---|
| 0 | 8 bytes | signature `89 45 42 4B 0D 0A 1A 0A` (`\x89EBK\r\n\x1A\n`; built like PNG's, to show damage done by text-mode transfer) |
| 8 | u8 | major version: 1 |
| 9 | u8 | minor version: 0 |
| 10 | u16 | flags; all reserved, must be 0 |
| 12 | u32 | reserved, must be 0 |

A reader must reject a file whose signature is wrong, whose major version it does not know, or whose flags or
reserved field are not 0.

A reader of version 1.0 must accept every minor version of major version 1. Later minor versions only add what a
reader can safely ignore or refuse member by member: optional index sections, storage modes, character-table
layouts. An optional section must not change the meaning of the required sections or of the bytes of a member.
(Versions 0.x were drafts and were not compatible with each other or with 1.0.)

### 3.2 Footer (16 bytes)

| Offset from the start of the footer | Type | Content |
|---|---|---|
| 0 | u32 | compressed length of the index, in bytes |
| 4 | u32 | length of the index after decompression, in bytes |
| 8 | u32 | CRC-32 of the decompressed index |
| 12 | 4 bytes | end signature `45 42 4B 1A` (`EBK\x1A`), to show that the file was cut short |

The index starts at: file length − 16 − compressed length of the index.

### 3.3 Opening a file

A reader must make these checks in this order and reject the whole file when one fails:

1. The file is at least 32 bytes long; read and check the header (3.1).
2. Read the footer and check the end signature; each of the two index lengths is at most 64 MiB, the compressed
   length is at most the file length − 32, and it meets the bound of section 2.
3. Read the index, decompress it as an exact brotli stream, check its CRC-32.
4. Parse the sections (section 5) with their own checks.
5. Cross-checks: the raw lengths of the text blocks add up to the length of the text stream (section 4); the
   equation at the head of this section holds; the character table is present exactly when a member uses it.

(Informative: when a file is read over a network by byte ranges, header and footer can be fetched at the same
time — for example the first 16 bytes and the last 64 KiB — and blocks and resource items as they are needed.)

## 4. The text stream and its blocks

The members stored "in the stream" (see 5.4), taken in the order of the member table, have their **stored bytes**
joined end to end. This is the **text stream**, of length L. The offset of a member in it is not recorded: it is
the sum of the stored lengths of the in-stream members before it. An in-stream member of length 0 takes no bytes
(its offset may equal L).

The text stream is cut into B **text blocks**; block i covers the `r_i` bytes that follow the block before it, and
`r_i` is recorded in the block table (5.2).

- Every block has `1 ≤ r_i ≤ 2^24` (16 MiB), and the `r_i` must add up to L. When L is 0, B must be 0.
- Every block except the last has `r_i ≥ 4096`. (Informative: each block has a fixed cost — an entry in the table,
  the start of a decompression. Without a minimum, a file could make a reader do work out of proportion to its
  content with a hundred thousand blocks of one byte. With it there are at most L div 4096 + 1 blocks.)
- The data of each block is an exact brotli stream of `r_i` bytes and meets the bound of section 2.
- A block boundary may fall inside a member or between members; the writer decides.
- Reading an in-stream member with a stored length other than 0 (offset o in the stream, stored length n) takes
  decompressing the blocks that cover `[o, o+n)`.

(Informative: block lengths vary so that a writer can cut at member boundaries — a chapter then lies in one
block — and can put the few members a reader needs first when opening a book into a small block of their own.
Section 9 describes what the reference writer does.)

## 5. The index

Decompressed, the index is a sequence of **sections**:

```
section := tag u8, length varint, content (length bytes)
```

- Sections are in ascending order of tag, and no tag appears twice.
- A tag below 0x80 marks a **required section**: a reader that does not know it must reject the file. A tag of
  0x80 or above marks an **optional section**: a reader that does not know it skips it.
- The content of a section must use exactly the length it declares, and the sections must use exactly the index.

### 5.1 Sections

| Tag | Name | Must be present |
|---|---|---|
| 0x01 | block table | yes |
| 0x02 | character table | when a member uses the code page (storage mode 1), and only then |
| 0x03 | member table | yes |

### 5.2 Block table (0x01)

```
number of blocks B     varint    at most 2^20, and at most half the bytes left in the section
B entries:
  raw length r_i       varint    1 to 2^24; at least 4096 in every entry but the last
  compressed length c_i varint   1 to r_i + r_i div 4096 + 64
```

Block i is at file offset 16 + the compressed lengths of the blocks before it, and at offset "the raw lengths of
the blocks before it" in the text stream.

### 5.3 Character table (0x02)

```
layout            u8      this version defines layout 1
characters        bytes   the rest of the section
```

In layout 1 the characters are valid UTF-8 text listing the non-ASCII characters that occur in the members that
use the code page. The position of a character in the list, counted from 0, is its **rank**; the rank decides how
it is coded (section 6). A reader must check that the text is valid UTF-8, that every character is above U+007F,
that none appears twice, and that there are at most 8128 of them; it rejects the file otherwise. There may be none.

A reader must check the number of bytes of the list (at most 4 × 8128) before it decodes it: the section can be
as long as the index.

A writer should list the characters by descending frequency, and characters of equal frequency by ascending code
point, so that writers agree on the table of a book. A reader does not depend on how the order came about.

A reader that does not know the layout must not reject the file; it must report "unsupported" for every member
of storage mode 1, and it does not look at the characters. The rule of 5.1 holds whatever the layout: a file with
a character table and no member of storage mode 1 must be rejected.

### 5.4 Member table (0x03)

```
number of members M    varint    at most 2^20, and at most the bytes left in the section divided by 9
M entries
```

M may be 0. Each entry:

```
storage mode     u8
path length      varint     in bytes, 1 to 1024
path             path length bytes
raw length       varint     length of the member's original bytes
stored length    varint     bytes the member takes in the text stream or in the resource area
CRC-32           u32        of the member's original bytes
```

All entries have the same layout, so a reader can parse the whole table when it does not know the storage mode
of some member.

| Mode | Where | Meaning of the stored bytes | What a reader must check |
|---|---|---|---|
| 0 | text stream | the original bytes | stored length = raw length |
| 1 | text stream | the original bytes recoded with the code page (section 6) | raw length ≤ 4 × stored length, stored length ≤ 2 × raw length |
| 2 | resource area | the original bytes | stored length = raw length |
| 3 | resource area | an exact brotli stream of the raw length | 1 ≤ stored length < raw length |
| 4 | resource area | a JPEG file, recompressed reversibly with Lepton (section 7) | 1 ≤ stored length < raw length ≤ 2^27, and raw length ≤ 8 × stored length |

- Members of modes 0 and 1 are **in-stream members**; the others are **resource items**. The two kinds may be
  mixed in the table.
- The two length rules of mode 1 are checked whatever the layout of the character table, also one the reader does
  not know: layouts defined later must meet them.
- A resource item is at file offset 16 + the length of the text blocks + the stored lengths of the resource items
  before it in the table.
- A member of length 0 uses mode 0 or 2, and its CRC-32 is 0.
- Modes 5 and above are reserved for later versions. A reader that does not know the mode of a member must not
  reject the file; it reports "unsupported" when that member is read. Such a member counts as a resource item
  when offsets are computed (so modes added later are resource items). A reader does not check how the raw and
  stored lengths of such a member relate — the version that defines the mode says — and the rule on length 0
  does not apply to it; but its stored length counts toward the offsets as usual, and the equation of section 3
  must hold. Reading such a member reports "unsupported", not "too large" on account of the length it declares.

The **path** is the path the member had in the EPUB container (for example `OEBPS/chapter1.xhtml`). A reader must
check of every path that

- it is valid UTF-8 and contains none of U+0000–U+001F, U+007F and the backslash;
- it does not begin or end with `/` and has no empty segment (two `/` in a row);
- no segment is `.` or `..`;
- it does not appear twice in the table;
- it is not a directory of another member's path (`a` and `a/b` cannot both be members);

and reject the file when one of these fails. Paths are compared byte for byte, without case folding and without
Unicode normalisation. (Informative: the format does not ask for uniqueness up to case or normalisation, because
that needs Unicode data tables, and readers with different tables would disagree on whether a file is valid.)

## 6. The code page (layout 1)

The code page rewrites valid UTF-8 text character by character into another sequence of bytes in which the
frequent non-ASCII characters take one or two bytes. It applies to members of storage mode 1 only, and a book has
one character table. A member whose original bytes are not valid UTF-8 must not use it (a writer stores such a
member in mode 0).

For every character (Unicode scalar value) c of the text:

| Condition | Output |
|---|---|
| c ≤ U+007F | 1 byte: c itself (0x00–0x7F) |
| c is in the table with rank r < 64 | 1 byte: `0x80 + r` (0x80–0xBF) |
| c is in the table with rank 64 ≤ r < 8128 | 2 bytes: `0xC0 + (r−64) div 128` (0xC0–0xFE), `0x80 + (r−64) mod 128` (0x80–0xFF) |
| c is not in the table (and c > U+007F) | 4 bytes: `0xFF`, `0x80 + (c >> 14)`, `0x80 + ((c >> 7) & 0x7F)`, `0x80 + (c & 0x7F)` |

A writer must use the first rule that applies (a character that is in the table must not be written in the
four-byte form).

Decoding starts at the first stored byte of the member; the first byte b of each form says which it is:

```
b < 0x80            → the character b
0x80 ≤ b ≤ 0xBF     → the character of rank b − 0x80
0xC0 ≤ b ≤ 0xFE     → one more byte t (must be ≥ 0x80): the character of rank 64 + (b − 0xC0)·128 + (t − 0x80)
b = 0xFF            → three more bytes x y z (each must be ≥ 0x80): the code point ((x−0x80) << 14) | ((y−0x80) << 7) | (z−0x80)
```

A reader must report an error and must not return the member when

- the stored bytes end inside a form of more than one byte, or a byte after the first is below 0x80;
- a rank is not below the number of characters in the table;
- the code point of a four-byte form is not above U+007F, is above U+10FFFF, lies in U+D800–U+DFFF, or is a
  character that is in the table;
- the UTF-8 length of the decoded text is not the raw length of the member, or its CRC-32 does not match.

Notes (informative):

- The byte ranges of layout 1 are chosen on purpose: one-byte codes lie in the range of UTF-8 continuation bytes
  and the first bytes of two-byte codes in the range of UTF-8 lead bytes, which matches the byte classes built
  into brotli. Measured savings against plain UTF-8: Chinese 9.5–9.8% (GB18030: 4%), Japanese and Korean 7.3–7.9%,
  other non-Latin scripts 4.6–10.3%, Vietnamese 3.2%, European languages in Latin script 0.3–1.7%.
- The coding is not self-synchronising: the range of the later bytes overlaps the other forms, so a member can
  only be decoded from its start.
- A writer should compare per book and use the code page only when the file comes out smaller; otherwise every
  member uses mode 0 and there is no character table. For a small book the table itself is a visible cost.
- The table holds at most 8128 characters and the rest take the four-byte form; of 334 Chinese books measured,
  one needed it.
- There are always 64 one-byte codes. With 112, Japanese and Korean save another 0.3–0.7%, but the two-byte codes
  then run short and Chinese gets worse; a later version can define another layout number for that.

## 7. Resource items

- **Mode 2**: stored as it is. For formats that are compressed already (PNG, GIF, WebP, WOFF2, audio, video).
- **Mode 3**: the whole member as one brotli stream. For binary data that is not compressed (TTF, OTF) and text
  that is not UTF-8.
- **Mode 4**: the stored bytes are a Lepton file — Lepton is a reversible recompression of JPEG designed at
  Dropbox — which decodes to the member's original JPEG file.
  - **Definition.** Lepton has no specification of its own; implementations define it. This specification refers
    to `lepton_jpeg` 0.5.8 (Microsoft's port to Rust, Apache-2.0). `third_party/lepton_jpeg/` in the repository of
    the reference implementation is a fork of it, maintained there: its changes concern which inputs are refused
    and how, never the format of the coded stream (they are listed in `EBK-CHANGES.md` there). A member of mode 4 is valid if and only if that
    implementation decodes its stored bytes to bytes whose length and CRC-32 are those of the member table.
    The decoder is run with the settings that crate calls `compat_lepton_vector_write`, which are those a writer
    encodes with — progressive files accepted, Huffman tables that the JPEG standard does not allow refused,
    quantisation tables with zeros refused, no side of the image over 16386 pixels, the two arithmetic variants
    chosen by the flags in the Lepton header — together with the limits given below.
    Files that a reader must accept, with the checksums of what they decode to, are in `crates/ebk/tests/data/`
    of the reference implementation (`*.lep`).
    A reader checks length and CRC-32, so another Lepton implementation that behaves differently can only make a
    member fail to read; it cannot make a reader return other bytes.
  - **Writers** must, before using mode 4 for a member, decode what they are about to store through the reader's
    decoding path and compare the result with the original byte for byte. When the encoder refuses the file, the
    decoding fails or crashes, the result differs, or nothing is saved, the member is stored in another mode.
    (Informative: seen in practice — progressive JPEG files without an end-of-image marker, which Lepton accepts
    and restores to other bytes; CMYK; sampling factors above two. The reference writer uses one partition,
    `max_partitions = 1`: the result is half a percentage point smaller than with eight, and a reader in
    WebAssembly has one thread.)
    A writer must not use mode 4 for an image of more than 2^26 pixels (width × height), for an image with less
    than one byte per 8×8 block, for a progressive image with more scans than the rule for readers below allows,
    or when the JPEG is more than eight times as long as the Lepton file; the rules for readers below make such
    members unreadable.
  - **Readers**:
    - The raw length is at most 2^27 (128 MiB), and the length recorded in the header of the Lepton file must be
      the raw length of the member. Output is limited to the raw length and its buffer grows with the data that
      arrives.
    - A reader must have a **limit on pixels** (width × height; 2^26 unless it chooses lower) and report "too
      large" above it. The decoder allocates the coefficients of the whole image from its dimensions, up to six
      bytes per pixel, before it reads any image data, so this limit is the limit on memory.
    - **Work in proportion to the bytes that exist.** The decoder allocates and decodes all coefficients of the
      image whether or not there is data for them, so the dimensions must be tied to real bytes: the number of
      8×8 blocks (width × height ÷ 64) must not exceed the raw length in bytes, and a file that breaks this is
      damaged. With the rule of the member table that the raw length is at most eight times the stored length,
      a stored byte accounts for at most eight blocks. (Informative: real images are far inside these bounds. Of
      4,421 JPEG files in the test corpus two have less than a byte per block, and Lepton shrinks none to less
      than 1/3.4.)
    - A progressive image is written out scan by scan, and every scan is a pass over the coefficients of the
      whole image, while a scan need not cost the file more than a few bytes. So the number of scans times the
      number of 8×8 blocks of all components must not exceed sixteen times the raw length, and a file that
      breaks this is damaged. (Informative: encoders write about ten scans; the three progressive files of the
      test corpus are far inside the bound.)
    - The header holds one list of thread handoffs with at most 16 entries, and their first rows do not
      decrease; a file with more lists, more entries or rows that go backwards is damaged. (Informative: each
      entry is decoded into an image of its own, so without this a header could ask for the image many times.)
    - The header of a Lepton file is a zlib stream holding the JPEG's own headers, restart information and the
      bytes after the image. Decompressed it must not be longer than the raw length plus 64 KiB; its buffers grow
      with the data, and failing to allocate them is an error. (Informative: the implementation referred to has no
      bound here as published; zlib expands a thousandfold and the header's records may repeat.)
    - The decoder is a large body of code that was not written for hostile input. Where a panic can be caught, it
      is reported as damaged data. In WebAssembly it is a trap: the host should start a new instance, count the
      member as failed without trying it again, and put a time limit on every read.

**Checking.** An interface that returns a whole member must check its raw length and CRC-32 before it returns.
An interface that delivers a member as a stream (for large audio or video in mode 2) may deliver while it reads,
and must check at the end of the stream and report a mismatch.

## 8. Limits

Readers parse files from sources that cannot be trusted, in a browser among other places. There are two kinds of limit.

**Limits of the format.** A writer must not exceed them and a reader must reject a file that does. All readers
reach the same verdict.

| What | Limit |
|---|---|
| compressed and decompressed length of the index | 64 MiB each |
| number of text blocks B | 2^20 |
| raw length of a text block | 1 to 2^24; at least 4096 except for the last block |
| compressed length of a text block or of the index | n + n div 4096 + 64, where n is the decompressed length |
| number of members M | 2^20 |
| length of a path | 1 to 1024 bytes |
| character table | 8128 characters |
| raw length of a member of mode 4 | 2^27, and eight times its stored length |

**Limits of a reader.** A reader must have them; it chooses the values. Exceeding one makes that read report "too
large"; the file is still valid.

- A limit on the raw length of a member (reference implementation: 1 GiB by default, 256 MiB in the web reader),
  checked before memory is allocated.
- The limit on pixels for mode 4 (section 7).
- A tool that reads every member (unpacking, verifying) must have a configurable **limit on total output**, and
  must read in the order of the member table and keep decompressed blocks, so that its work is in proportion to
  the length of the text stream. (Informative: a brotli stream of a dozen bytes decompresses to 16 MiB, and a file
  of a few megabytes can declare thousands of gigabytes of text.) The limit counts the raw length of each member,
  including members that fail to read (the work may have been done), and not members stored in a mode the reader
  does not support.
- A reader must remember a block that failed to decompress and must not decompress it again; otherwise, when a
  damaged 16 MiB block holds thousands of small members, every one of them decompresses it once more. The same
  goes for a block that failed for lack of memory: it should not be tried again for every member (the reference
  implementation tries again when the caller asks).
- Running out of memory must be an error that is returned, also for allocations inside a decompressor (window,
  Huffman tables). (Informative: general-purpose decompression libraries tend to abort the process, which in
  WebAssembly is a trap.)
- Running out of memory while opening a file should be reported as "too large", not as "invalid": the same file is
  valid where there is more memory. A reader should keep little memory per member (reference implementation: 40
  bytes and the path).
- "Too large" is a verdict on the reader's circumstances, not a property of the file, so readers may differ: for
  a member that declares 2^32 bytes or more, a 32-bit reader reports "too large" where a 64-bit reader may find
  the data damaged. Neither may report the file as invalid, and neither may return wrong bytes.
- A reader must not allocate memory of a length declared in the index just because it is declared; output
  buffers grow with the data actually produced.
- A reader should keep at least two decompressed blocks (informative).

**Scope of an error.** An error in the index — any check of 3.3 — rejects the whole file. An error in the data — a
block that does not decompress, a CRC-32 that does not match, a code-page or Lepton decoding error — makes the
members concerned fail to read, and the reader may go on serving the others.

**Tools that write members to disk** (such as `extract`) must, beyond the path checks of 5.4,

- write only into a new or empty directory;
- create files exclusively, so that two paths that are the same up to case or Unicode normalisation give an error
  instead of one overwriting the other;
- refuse a path with a segment that contains one of `: * ? " < > |`, that ends in `.` or a space, or whose name
  without extension is, in any case, one of `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM0`–`COM9`,
  `LPT0`–`LPT9`, or `COM` or `LPT` followed by one of the superscript digits `¹ ² ³` (on Windows these name drives
  or devices, or are silently renamed);
- refuse a segment of more than 255 bytes (the limit of the common file systems);
- check everything the index can tell — the path rules above, unsupported storage modes, the member limit, the
  limit on total output — before writing the first file, and say how much was written when a later step fails.

**Tools that show paths to a person** should escape characters that cannot be seen (U+0080–U+009F, bidirectional
controls and the like): the path rules exclude only the ASCII control characters, and the others can change what
a terminal shows.

## 9. Making an EBK file from an EPUB

Requirements:

- The members are those of the **central directory** of the ZIP file: every entry of the directory becomes a
  member, whose path is the bytes of the file name in the directory record, as they are. Names are taken as
  UTF-8 whatever the "names are UTF-8" flag (bit 11) says: EPUB requires UTF-8 names and many tools do not set
  the flag; a reader that honours the flag and reads such a name as CP437 gets another name, and is deliberately
  not followed. A **directory entry** is an entry whose name ends in `/`. It is not kept; its name without the
  final `/` must meet the path rules of 5.4, its length and CRC-32 must be 0, and it must have no data (a
  compressed length of 0 when stored, an empty stream when deflated). Every other entry — also one whose name
  ends in a backslash — is a file.
- The input must be an EPUB: a ZIP file without `META-INF/container.xml` is not converted.
- To decide the order of the members a converter parses `container.xml` and the package document. They come from
  the same untrusted file: before parsing, the expansion of entities and the depth of nested elements must be
  bounded. (The reference converter does not parse a document that declares entities or nests deeper than 256
  elements, and keeps the order of the ZIP file. A few hundred bytes of XML can expand to gigabytes, or overflow
  the stack of a recursive parser.)
- ZIP readers disagree about the cases below (some read the directory at the end of the file, some read the local
  headers from the start), so the conversion must fail on them and must not guess or rename. The principle:
  **reading by the directory and reading from the start must give the same entries with the same bytes.**
  - **End record.** It is the end-of-central-directory record whose comment ends exactly at the end of the file.
    No such place, or more than one (a comment can hold a complete directory and end record), is refused. Also
    refused: an end record whose "entries on this disk" and "total entries" differ; a central directory that does
    not lie directly before the end record (before the ZIP64 end record, when there is one) or whose length does
    not fit its number of entries; ZIP64 end records that contradict each other, or a ZIP64 end record that is
    not 56 bytes long (one with extension data); multi-part archives.
  - **Names.** A name that is not valid UTF-8 or breaks the path rules of 5.4; two entries with the same name; an
    entry with an Info-ZIP Unicode Path extra field (0x7075), in the directory record or the local header, that
    holds another name.
  - **Local headers** that disagree with the directory record: another name or compression method; without a
    data descriptor (flag bit 3 of the local header; the bit in the directory record is not looked at), another
    CRC-32 or another of the two lengths. With a data descriptor: the CRC-32 and the two lengths in the local
    header must all be 0 or all be those of the directory record; the descriptor must follow the data directly
    and be 12, 16, 20 or 24 bytes long (with or without its signature, with lengths of 32 or 64 bits), and its
    CRC-32 and lengths must be those of the directory record. In an entry that is stored (not compressed), has a
    data descriptor, and has zeros in its local header, a reader going through the file can find the end of the
    data only by the descriptor's signature, so the data must not contain `PK\x07\x08`. The two lengths of a
    stored entry must be equal.
  - **Layout.** All entries, directory entries included, must lie one after the other from the start of the file
    to the central directory, with nothing between them but data descriptors, and must not overlap. This excludes
    local entries that the directory does not list, data before the first entry, and overlapping entries.
  - **Data.** An encrypted entry; a compression method other than "stored" and "deflate"; a deflate stream that
    does not end exactly at the compressed length (bytes after it, or no final block); an actual length or
    CRC-32 other than the directory record's.
- A converter must have a configurable **limit on input** — the total length of all entries once decompressed
  (reference implementation: 4 GiB by default) — and check it against the lengths declared in the directory
  before reading any entry. While reading an entry it must not allocate memory of the declared length in advance,
  and it stops with an error one byte past the declared length. (Informative: a ZIP file of 300 KB can decompress
  to 300 MiB.)
- `mimetype` and the files under `META-INF/` are kept as ordinary members.
- After writing, a converter must read every member back and compare it with the bytes in the EPUB, and only then
  let the output appear under its name (it writes a temporary file and renames it). It must not overwrite its input.

What the reference writer does (informative):

- **Which members go into the text stream.** Members whose bytes are valid UTF-8. The others are resource items:
  those that begin with `FF D8 FF` (JPEG, whatever the name) are tried in mode 4; other formats that are
  compressed already (by file name extension) are stored in mode 2; the rest are tried with brotli and stored in
  mode 3 when that is smaller. One exception: an entry that the EPUB's ZIP file holds in fewer bytes than it has
  can evidently still be compressed, and is tried with brotli even if it is of an already compressed format or a
  JPEG that Lepton could not take. Without this an EBK file can come out larger than the EPUB (seven books of the
  test corpus did, by 1–15%, on account of poorly compressed PNG files and nearly silent MP3 files).
- **Code page.** In-stream members that are valid UTF-8 with at least one non-ASCII character can use mode 1;
  a member of plain ASCII would be coded to the same bytes and stays in mode 0, which saves a decoding pass when
  it is read. The text stream is cut into blocks and compressed once without and once with the code page, and the
  total lengths, each with its index, are compared; the code page is used when it gives the smaller file. The
  price is twice the compression time for text.
- **Order.** `mimetype`, the files under `META-INF/`, the package document, the navigation document, style sheets,
  then the content documents in spine order, then everything else. The order does not affect the validity of a
  file and affects its size very little (0.04–0.27 percentage points).
- **Cutting blocks.** The target block size is 4 MiB by default. A text stream no longer than the target is one
  block. Otherwise the members before the spine get a first block of their own; after that a block ends at a
  member boundary, a new one starting when the next member does not fit; a member longer than the target is cut
  every target bytes. A cut that would leave a block shorter than 4096 bytes is not made: a first block shorter
  than that runs on to the first member boundary that brings it to 4096 bytes, and elsewhere the block runs on to
  the target size.
- **Block size.** Over the whole corpus, 4 MiB costs 0.1–0.2 percentage points against compressing each book as
  one stream, but most books fit in one block; on the books that are cut, the cost is 3.1–3.3 points (2.5 points
  between 4 MiB and 16 MiB on long novels). The target can be set as high as 16 MiB.

## 10. Use with an EPUB renderer (informative)

A renderer needs members by path: as text, as bytes, and their length. The length comes from the member table
without decompressing anything. foliate-js reads a book — metadata, sections, table of contents, pages — through
three functions that provide exactly this.

The media type of a member is not recorded in the container; the manifest of the package document says it.

## 11. Conformance

- A **reader** must meet everything sections 2 to 8 require of readers — the form of varints, exact brotli
  streams and the bound on their length, the file structure, the text blocks, the index, the decoding of the code
  page, the rules for mode 4, the limits — and must support storage modes 0, 1 (layout 1), 2, 3 and 4.
- A **writer** must write only files that pass all of these checks.
- A **converter** that makes EBK files from EPUB must also meet section 9.

## 12. Decisions (informative)

1. **The member table records no media types.** The package document has them; two copies raise the question of
   which one counts. An optional section can add them later.
2. **CRC-32 as the checksum.** Without a signature, SHA-256 would not prevent tampering either, and 2^-32 is
   enough to find a restoration error. The format explicitly gives no protection against tampering.
3. **No content identifier for a book.** Hashing a file needs no support from the format. A digest for
   deduplication across editions can be defined in an optional section.
4. **Blocks of at most 16 MiB raw.** This is brotli's largest window, and it bounds a reader's memory and latency.
5. **64 one-byte codes in the code page**, and block lengths that vary.
6. **Lepton for JPEG**, defined by reference to one implementation (section 7), instead of JPEG XL: a reading
   system always hands the restored JPEG to the renderer and has no use for JPEG XL as an image format, and the
   Rust implementations of JPEG XL that restore JPEG files were less mature when this was decided.
7. **A minimum block length of 4096 bytes** except for the last block.
8. **The converter parses the EPUB's ZIP file itself and strictly**, so that an EBK file does not depend on which
   ZIP library read the EPUB.

Open (none of these changes the format): the share of progressive JPEG files in real libraries, which Lepton
often cannot take; the best default block size for long books; the allocation of tags for optional sections
(0x80 and up, none assigned yet — shelf metadata, checksums per block, content digests and signatures are
candidates).
