//! Reading the members of an EPUB and deciding the order they go into an EBK file.
//!
//! The ZIP file is parsed here rather than by a ZIP library, so that there is one reading of a file
//! and it is the strict one: a member's path is the name in the central directory, byte for byte,
//! and anything that ZIP readers are known to disagree about is an error (spec section 9).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{bail, ensure, Context, Result};

pub struct Entry {
    pub path: String,
    pub data: Vec<u8>,
    /// Whether the ZIP file held this entry in fewer bytes than it has.
    pub deflated: bool,
}

struct Record {
    path: String,
    method: u16,
    crc32: u32,
    packed_len: u64,
    len: u64,
    data_at: u64,
}

/// The file entries of a ZIP archive, in the order of its central directory. Directory entries are left out.
pub struct Archive {
    file: File,
    records: Vec<Record>,
}

fn u16_at(buf: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([buf[at], buf[at + 1]])
}

fn u32_at(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(buf[at..at + 4].try_into().unwrap())
}

fn u64_at(buf: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(buf[at..at + 8].try_into().unwrap())
}

fn read_at(file: &mut File, offset: u64, len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut buf).context("the ZIP file ends too early")?;
    Ok(buf)
}

const ENCRYPTED: u16 = 0x2041;
const HAS_DESCRIPTOR: u16 = 0x0008;
const ZIP64: u16 = 0x0001;
const UNICODE_PATH: u16 = 0x7075;

/// The fields of a header's extra area: (id, body). A field cut off by the end of the area ends the list.
fn extra_fields(mut area: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    std::iter::from_fn(move || {
        let (id, size) = (u16_at(area.get(..4)?, 0), u16_at(area, 2) as usize);
        let body = area.get(4..4 + size)?;
        area = &area[4 + size..];
        Some((id, body))
    })
}

/// What one entry occupies in the file, for the check that the entries lie one after the other.
struct Extent {
    start: u64,
    /// End of the entry's data; a data descriptor may follow.
    end: u64,
    has_descriptor: bool,
    crc32: u32,
    packed_len: u64,
    len: u64,
    name: usize,
}

impl Archive {
    /// Reads the central directory and checks every local header against it.
    /// Fails when the entries together declare more than `max_total` bytes.
    pub fn open(path: &Path, max_total: u64) -> Result<Archive> {
        let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        let (directory_at, directory_len, count) = end_record(&mut file).context("not a ZIP file")?;
        // every record takes at least 46 bytes of the file, so this allocation is no larger than the file
        let directory = read_at(&mut file, directory_at, usize::try_from(directory_len).context("not a ZIP file")?)?;

        let (mut records, mut extents, mut names, mut seen) = (Vec::new(), Vec::new(), Vec::new(), HashSet::new());
        let (mut at, mut total) = (0usize, 0u64);
        for n in 0..count {
            const FIXED: usize = 46;
            ensure!(directory.len() - at >= FIXED && directory[at..].starts_with(b"PK\x01\x02"), "ZIP directory entry {n} is damaged");
            let rec = &directory[at..];
            let (flags, method, crc32) = (u16_at(rec, 8), u16_at(rec, 10), u32_at(rec, 16));
            let (name_len, extra_len, comment_len) = (u16_at(rec, 28) as usize, u16_at(rec, 30) as usize, u16_at(rec, 32) as usize);
            let next = FIXED + name_len + extra_len + comment_len;
            ensure!(rec.len() >= next, "ZIP directory entry {n} is damaged");
            let name = &rec[FIXED..FIXED + name_len];
            let extra = &rec[FIXED + name_len..FIXED + name_len + extra_len];
            at += next;

            let shown = String::from_utf8_lossy(name);
            let Ok(path) = std::str::from_utf8(name) else { bail!("ZIP entry {n} has a name that is not UTF-8: {shown:?}") };
            ensure!(seen.insert(path), "two ZIP entries are named {path:?}");
            ensure!(flags & ENCRYPTED == 0, "{path:?} is encrypted");
            let (mut len, mut packed_len, mut header_at, mut disk) =
                (u64::from(u32_at(rec, 24)), u64::from(u32_at(rec, 20)), u64::from(u32_at(rec, 42)), u32::from(u16_at(rec, 34)));
            for (id, mut body) in extra_fields(extra) {
                match id {
                    // the values that do not fit their 32-bit fields, in this order
                    ZIP64 => {
                        let mut take = |saturated: bool| -> Result<Option<u64>> {
                            if !saturated {
                                return Ok(None);
                            }
                            ensure!(body.len() >= 8, "{path:?} has a damaged ZIP64 field");
                            let v = u64_at(body, 0);
                            body = &body[8..];
                            Ok(Some(v))
                        };
                        len = take(len == 0xFFFF_FFFF)?.unwrap_or(len);
                        packed_len = take(packed_len == 0xFFFF_FFFF)?.unwrap_or(packed_len);
                        header_at = take(header_at == 0xFFFF_FFFF)?.unwrap_or(header_at);
                        if disk == 0xFFFF {
                            ensure!(body.len() >= 4, "{path:?} has a damaged ZIP64 field");
                            disk = u32_at(body, 0);
                        }
                    }
                    // some readers use this name instead of the one in the header
                    UNICODE_PATH => ensure!(body.get(5..) == Some(name), "{path:?} carries a second, different name (Unicode Path extra field)"),
                    _ => {}
                }
            }
            ensure!(disk == 0, "{path:?} is on another disk of a multi-part ZIP");
            ensure!(method == 0 || method == 8, "{path:?} uses compression method {method}, which is not supported");
            ensure!(method == 8 || packed_len == len, "{path:?} is stored without compression but has two different lengths");

            // the local header: readers that go through the file from the start see only this one
            let local = read_at(&mut file, header_at, 30).with_context(|| format!("cannot read {path:?}"))?;
            ensure!(local.starts_with(b"PK\x03\x04"), "{path:?} has no local header where the directory says");
            let local_flags = u16_at(&local, 6);
            ensure!(local_flags & ENCRYPTED == 0, "{path:?} is encrypted");
            ensure!(u16_at(&local, 8) == method, "{path:?} has another compression method in its local header");
            let (local_name_len, local_extra_len) = (u16_at(&local, 26) as usize, u16_at(&local, 28) as usize);
            let rest = read_at(&mut file, header_at + 30, local_name_len + local_extra_len).with_context(|| format!("cannot read {path:?}"))?;
            let (local_name, local_extra) = rest.split_at(local_name_len);
            ensure!(local_name == name, "{path:?} has another name in its local header");
            let has_descriptor = local_flags & HAS_DESCRIPTOR != 0;
            let (mut local_len, mut local_packed_len) = (u64::from(u32_at(&local, 22)), u64::from(u32_at(&local, 18)));
            for (id, body) in extra_fields(local_extra) {
                match id {
                    // here both lengths are present, or neither
                    ZIP64 if body.len() >= 16 => {
                        if local_len == 0xFFFF_FFFF {
                            local_len = u64_at(body, 0);
                        }
                        if local_packed_len == 0xFFFF_FFFF {
                            local_packed_len = u64_at(body, 8);
                        }
                    }
                    UNICODE_PATH => ensure!(body.get(5..) == Some(name), "{path:?} carries a second, different name (Unicode Path extra field)"),
                    _ => {}
                }
            }
            // With a data descriptor the values follow the data (checked below) and the header's fields are left at
            // zero - or filled in all the same. Anything else would send a reader that trusts them elsewhere.
            let in_header = u32_at(&local, 14) == crc32 && local_len == len && local_packed_len == packed_len;
            let left_empty = has_descriptor && u32_at(&local, 14) == 0 && local_len == 0 && local_packed_len == 0;
            ensure!(in_header || left_empty, "{path:?} has another length or checksum in its local header");
            let data_at = header_at + 30 + (local_name_len + local_extra_len) as u64;
            let Some(end) = data_at.checked_add(packed_len).filter(|&end| end <= directory_at) else { bail!("{path:?} reaches into the ZIP directory") };
            // A reader going through the file finds the end of a deflate stream by itself. Of an entry that is
            // stored it knows only the descriptor's signature, so that must not occur in the data.
            if method == 0 && left_empty && packed_len > 0 {
                let data = read_at(&mut file, data_at, usize::try_from(packed_len).context("an entry is too long")?)?;
                ensure!(!data.windows(4).any(|w| w == b"PK\x07\x08"), "{path:?} is stored with a data descriptor and holds the descriptor's signature");
            }
            extents.push(Extent { start: header_at, end, has_descriptor, crc32, packed_len, len, name: names.len() });
            names.push(path);

            // a directory entry: nothing to keep, and nothing in it
            if let Some(directory) = path.strip_suffix('/') {
                if let Err(why) = ebk::check_path(directory) {
                    bail!("{path:?}: {why}");
                }
                ensure!(len == 0 && crc32 == 0, "{path:?} is a directory entry with content");
                let packed = read_at(&mut file, data_at, usize::try_from(packed_len).context("an entry is too long")?)?;
                let empty = if method == 0 { packed.is_empty() } else { inflate(&packed, 0).is_ok() };
                ensure!(empty, "{path:?} is a directory entry with content");
                continue;
            }
            total = total.saturating_add(len);
            ensure!(total <= max_total, "the EPUB unpacks to more than {max_total} bytes (raise --max-input to go on)");
            records.push(Record { path: path.to_owned(), method, crc32, packed_len, len, data_at });
        }
        ensure!(at == directory.len(), "the ZIP directory is longer than its {count} entries");

        // The entries must lie one after the other from the start of the file to the directory, with nothing
        // between them but data descriptors: a reader that goes through the file then finds the same entries.
        extents.sort_unstable_by_key(|e| e.start);
        let mut previous: Option<&Extent> = None;
        for next in extents.iter().map(Some).chain([None]) {
            let (start, what) = match next {
                Some(e) => (e.start, format!("before {:?}", names[e.name])),
                None => (directory_at, "before the ZIP directory".to_owned()),
            };
            let Some(gap) = start.checked_sub(previous.map_or(0, |p| p.end)) else { bail!("two ZIP entries overlap ({what})") };
            match previous {
                Some(p) if p.has_descriptor => {
                    // crc, packed length, length; with or without a signature, with 32-bit or 64-bit lengths
                    ensure!(matches!(gap, 12 | 16 | 20 | 24), "the ZIP file has {gap} bytes that belong to no entry {what}");
                    let d = read_at(&mut file, p.end, gap as usize)?;
                    let is = |at: usize, wide: bool| {
                        let (packed_len, len) = if wide { (u64_at(&d, at + 4), u64_at(&d, at + 12)) } else { (u32_at(&d, at + 4).into(), u32_at(&d, at + 8).into()) };
                        u32_at(&d, at) == p.crc32 && packed_len == p.packed_len && len == p.len
                    };
                    let signed = d.starts_with(b"PK\x07\x08");
                    let agrees = match gap {
                        12 => is(0, false),
                        16 => signed && is(4, false),
                        20 => is(0, true),
                        _ => signed && is(4, true),
                    };
                    ensure!(agrees, "{:?} has another length or checksum in its data descriptor", names[p.name]);
                }
                _ => ensure!(gap == 0, "the ZIP file has {gap} bytes that belong to no entry {what}"),
            }
            previous = next;
        }
        Ok(Archive { file, records })
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn path(&self, i: usize) -> &str {
        &self.records[i].path
    }

    /// Whether entry `i` takes fewer bytes in the ZIP file than it has.
    pub fn deflated(&self, i: usize) -> bool {
        self.records[i].packed_len < self.records[i].len
    }

    /// The bytes of entry `i`, checked against the length and CRC-32 in the directory.
    pub fn read(&mut self, i: usize) -> Result<Vec<u8>> {
        let r = &self.records[i];
        let path = &r.path;
        // open() checked that these bytes are in the file, so the buffer is no larger than the file
        let packed = read_at(&mut self.file, r.data_at, r.packed_len as usize).with_context(|| format!("cannot read {path:?}"))?;
        let data = match r.method {
            0 => packed,
            _ => inflate(&packed, r.len).map_err(|why| anyhow::anyhow!("{path:?} {why}"))?,
        };
        ensure!(data.len() as u64 == r.len, "{path:?} does not have the length the ZIP directory declares");
        ensure!(crc32fast::hash(&data) == r.crc32, "{path:?} does not match its checksum");
        Ok(data)
    }
}

/// Inflates a raw deflate stream that must end exactly at the end of `packed` and give `len` bytes.
/// The output grows with what comes out; nothing is reserved from the declared length.
fn inflate(packed: &[u8], len: u64) -> Result<Vec<u8>, &'static str> {
    use flate2::{Decompress, FlushDecompress, Status};
    const LENGTH: &str = "does not have the length the ZIP directory declares";
    let mut inflater = Decompress::new(false);
    let mut out: Vec<u8> = Vec::new();
    loop {
        if out.len() == out.capacity() {
            // one byte past the declared length is enough to see that there is more
            let room = (len.saturating_add(1) - out.len() as u64).min(out.len().max(1 << 16) as u64);
            if room == 0 {
                return Err(LENGTH);
            }
            out.try_reserve_exact(room as usize).map_err(|_| "is too large for the memory there is")?;
        }
        let before = (inflater.total_in(), inflater.total_out());
        let status = inflater.decompress_vec(&packed[inflater.total_in() as usize..], &mut out, FlushDecompress::None);
        match status {
            Ok(Status::StreamEnd) => break,
            Ok(_) if (inflater.total_in(), inflater.total_out()) != before => {}
            // no progress although there was room: the input ran out before the stream ended
            Ok(_) if out.len() < out.capacity() => return Err("has compressed data that ends too early"),
            Ok(_) => {}
            Err(_) => return Err("has damaged compressed data"),
        }
    }
    if inflater.total_in() != packed.len() as u64 {
        return Err("has bytes after the end of its compressed data");
    }
    if out.len() as u64 != len {
        return Err(LENGTH);
    }
    Ok(out)
}

/// Finds the central directory: (offset, length, number of entries).
fn end_record(file: &mut File) -> Result<(u64, u64, u64)> {
    const LEN: usize = 22;
    let file_len = file.metadata()?.len();
    let tail_len = file_len.min(LEN as u64 + 0xFFFF); // the record and the longest archive comment
    let tail = read_at(file, file_len - tail_len, tail_len as usize)?;
    // The record is the one whose comment ends exactly at the end of the file. A comment can hold bytes
    // that look like another record; when two places qualify, readers differ in which they take.
    let mut found = (0..tail.len().saturating_sub(LEN - 1)).filter(|&at| tail[at..].starts_with(b"PK\x05\x06") && at + LEN + u16_at(&tail, at + 20) as usize == tail.len());
    let Some(at) = found.next() else { bail!("no end-of-central-directory record at the end of the file") };
    ensure!(found.next().is_none(), "more than one place could be the end-of-central-directory record");
    drop(found);
    let end_at = file_len - tail_len + at as u64;
    let rec = &tail[at..];
    let (disk, directory_disk) = (u32::from(u16_at(rec, 4)), u32::from(u16_at(rec, 6)));
    let (on_disk, count) = (u64::from(u16_at(rec, 8)), u64::from(u16_at(rec, 10)));
    let (len, offset) = (u64::from(u32_at(rec, 12)), u64::from(u32_at(rec, 16)));

    // ZIP64: a locator directly before the record points at the record with the real values
    let locator_at = end_at.checked_sub(20);
    let locator = match locator_at {
        Some(locator_at) => Some(read_at(file, locator_at, 20)?).filter(|l| l.starts_with(b"PK\x06\x07")),
        None => None,
    };
    let (disk, directory_disk, on_disk, count, len, offset, directory_end) = match locator {
        Some(locator) => {
            let at = u64_at(&locator, 8);
            ensure!(u32_at(&locator, 4) == 0 && u32_at(&locator, 16) <= 1, "multi-part ZIP files are not supported");
            let rec = read_at(file, at, 56)?;
            ensure!(rec.starts_with(b"PK\x06\x06"), "the ZIP64 end-of-central-directory record is missing");
            ensure!(at.checked_add(56) == locator_at && u64_at(&rec, 4) == 44, "the ZIP64 end-of-central-directory record is not where it should be");
            let (count64, len64, offset64) = (u64_at(&rec, 32), u64_at(&rec, 40), u64_at(&rec, 48));
            // some readers use the ZIP64 values only for the fields that are saturated: the two must not differ
            let agree = |old: u64, saturated: u64, new: u64| old == saturated || old == new;
            ensure!(agree(count, 0xFFFF, count64) && agree(on_disk, 0xFFFF, u64_at(&rec, 24)) && agree(len, 0xFFFF_FFFF, len64) && agree(offset, 0xFFFF_FFFF, offset64),
                    "the two end-of-central-directory records disagree");
            (u32_at(&rec, 16), u32_at(&rec, 20), u64_at(&rec, 24), count64, len64, offset64, at)
        }
        // without ZIP64 the fields mean what they say; 0xFFFF is then simply 65535 entries
        None => (disk, directory_disk, on_disk, count, len, offset, end_at),
    };
    ensure!(disk == 0 && directory_disk == 0, "multi-part ZIP files are not supported");
    // readers disagree on which of the two counts they use
    ensure!(on_disk == count, "the ZIP file gives two different numbers of entries ({on_disk} and {count})");
    ensure!(offset.checked_add(len) == Some(directory_end), "the ZIP directory is not where the end record says");
    ensure!(count <= len / 46, "the ZIP directory is too short for {count} entries");
    Ok((offset, len, count))
}

/// The files of an EPUB in ZIP directory order.
pub fn read(path: &Path, max_total: u64) -> Result<Vec<Entry>> {
    let mut archive = Archive::open(path, max_total)?;
    let mut entries = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        entries.push(Entry { path: archive.path(i).to_owned(), data: archive.read(i)?, deflated: archive.deflated(i) });
    }
    Ok(entries)
}

const SPINE: u32 = 5;

/// Indices of `entries` in the order a reader needs them when opening the book:
/// mimetype, META-INF, the package document, navigation, style sheets, the spine, then the rest.
/// Also returns how many of them come before the spine.
/// An EPUB whose package document cannot be parsed keeps its ZIP order after META-INF.
pub fn reading_order(entries: &[Entry]) -> (Vec<usize>, usize) {
    let mut rank: HashMap<String, (u32, usize)> = package_ranks(entries).unwrap_or_default();
    for e in entries {
        if e.path == "mimetype" {
            rank.insert(e.path.clone(), (0, 0));
        } else if e.path.starts_with("META-INF/") {
            rank.insert(e.path.clone(), (1, 0));
        }
    }
    let rank_of = |i: usize| rank.get(&entries[i].path).copied().unwrap_or((SPINE + 1, 0));
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&i| (rank_of(i), i));
    let head = order.iter().take_while(|&&i| rank_of(i).0 < SPINE).count();
    (order, head)
}

/// Parses `container.xml` or a package document. They come out of the EPUB as they are, so what the
/// parser may be made to do is bounded first; a document outside the bounds is not parsed, and the book
/// keeps its ZIP order.
fn parse_xml(xml: &str) -> Option<roxmltree::Document<'_>> {
    // Entities multiply: a few hundred bytes that declare and reference them expand to gigabytes of nodes.
    // The parser goes one stack frame deeper for every element inside another, without a limit of its own.
    const MAX_DEPTH: usize = 256;
    if xml.contains("<!ENTITY") || nesting(xml) > MAX_DEPTH {
        return None;
    }
    // without entities a node takes at least a byte of the document
    let nodes_limit = u32::try_from(xml.len()).unwrap_or(u32::MAX);
    roxmltree::Document::parse_with_options(xml, roxmltree::ParsingOptions { allow_dtd: true, nodes_limit, ..Default::default() }).ok()
}

/// How deep the elements of a document are nested, at most. Never less than the parser will find:
/// what is not understood here counts as opening an element.
fn nesting(xml: &str) -> usize {
    let b = xml.as_bytes();
    let skip_past = |from: usize, end: &[u8]| b[from..].windows(end.len()).position(|w| w == end).map_or(b.len(), |at| from + at + end.len());
    let (mut depth, mut deepest, mut i) = (0usize, 0usize, 0);
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
        } else if b[i..].starts_with(b"<!--") {
            i = skip_past(i + 4, b"-->");
        } else if b[i..].starts_with(b"<![CDATA[") {
            i = skip_past(i + 9, b"]]>");
        } else if b[i..].starts_with(b"<?") {
            i = skip_past(i + 2, b"?>");
        } else if b[i..].starts_with(b"<!") {
            i += 2; // a declaration; what is inside it is looked at like the rest
        } else {
            // a tag: to its end, over quoted attribute values
            let (mut j, mut quote) = (i + 1, 0u8);
            while j < b.len() && (quote != 0 || b[j] != b'>') {
                if quote != 0 {
                    quote = if b[j] == quote { 0 } else { quote };
                } else if b[j] == b'"' || b[j] == b'\'' {
                    quote = b[j];
                }
                j += 1;
            }
            if b.get(i + 1) == Some(&b'/') {
                depth = depth.saturating_sub(1);
            } else if b.get(j.wrapping_sub(1)) != Some(&b'/') {
                depth += 1;
                deepest = deepest.max(depth);
            }
            i = j + 1;
        }
    }
    deepest
}

fn package_ranks(entries: &[Entry]) -> Option<HashMap<String, (u32, usize)>> {
    let text = |path: &str| {
        let data = &entries.iter().find(|e| e.path == path)?.data;
        std::str::from_utf8(data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data)).ok()
    };
    let parse = parse_xml;
    let container = parse(text("META-INF/container.xml")?)?;
    let opf_path = container.descendants().find(|n| n.has_tag_name_local("rootfile"))?.attribute("full-path")?.to_owned();
    let opf = parse(text(&opf_path)?)?;
    let base = opf_path.rsplit_once('/').map_or("", |(dir, _)| dir);

    let mut ranks = HashMap::new();
    let mut by_id = HashMap::new();
    for item in opf.descendants().filter(|n| n.has_tag_name_local("item")) {
        let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) else { continue };
        let path = resolve(base, href);
        let media = item.attribute("media-type").unwrap_or("");
        if item.attribute("properties").is_some_and(|p| p.split_whitespace().any(|w| w == "nav")) || media == "application/x-dtbncx+xml" {
            ranks.insert(path.clone(), (3, 0));
        } else if media == "text/css" {
            ranks.insert(path.clone(), (4, 0));
        }
        by_id.insert(id, path);
    }
    for (n, itemref) in opf.descendants().filter(|n| n.has_tag_name_local("itemref")).enumerate() {
        if let Some(path) = itemref.attribute("idref").and_then(|id| by_id.get(id)) {
            ranks.entry(path.clone()).or_insert((SPINE, n));
        }
    }
    ranks.insert(opf_path, (2, 0));
    Some(ranks)
}

trait LocalName {
    fn has_tag_name_local(&self, name: &str) -> bool;
}

impl LocalName for roxmltree::Node<'_, '_> {
    fn has_tag_name_local(&self, name: &str) -> bool {
        self.is_element() && self.tag_name().name() == name
    }
}

/// The container path a manifest href points to: fragment dropped, percent-escapes decoded, '.' and '..' applied.
fn resolve(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or("");
    let mut bytes = Vec::with_capacity(href.len());
    let mut rest = href.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        let hex = tail.get(..2).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b, hex) {
            (b'%', Some(value)) => {
                bytes.push(value);
                rest = &tail[2..];
            }
            _ => {
                bytes.push(b);
                rest = tail;
            }
        }
    }
    let href = String::from_utf8_lossy(&bytes);
    let mut segments: Vec<&str> = if href.starts_with('/') { Vec::new() } else { base.split('/').filter(|s| !s.is_empty()).collect() };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            seg => segments.push(seg),
        }
    }
    segments.join("/")
}

#[cfg(test)]
mod tests {
    use super::{nesting, parse_xml, reading_order, Entry};

    fn entry(path: &str, data: &str) -> Entry {
        Entry { path: path.to_owned(), data: data.as_bytes().to_vec(), deflated: false }
    }

    fn order(entries: &[Entry]) -> (Vec<&str>, usize) {
        let (order, head) = reading_order(entries);
        (order.into_iter().map(|i| entries[i].path.as_str()).collect(), head)
    }

    const CONTAINER: &str = r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/pkg/content.opf"/></rootfiles></container>"#;

    #[test]
    fn what_a_reader_opens_first_comes_first_and_the_spine_follows_in_its_order() {
        let opf = r#"<package xmlns="http://www.idpf.org/2007/opf"><manifest>
            <item id="c2" href="../text/two.xhtml" media-type="application/xhtml+xml"/>
            <item id="c1" href="../text/one%20a.xhtml" media-type="application/xhtml+xml"/>
            <item id="css" href="style.css" media-type="text/css"/>
            <item id="nav" href="nav.xhtml" properties="nav" media-type="application/xhtml+xml"/>
            <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
            <item id="img" href="/cover.jpg" media-type="image/jpeg"/>
            <item id="out" href="../../../outside.xhtml" media-type="application/xhtml+xml"/>
            </manifest><spine><itemref idref="c1"/><itemref idref="missing"/><itemref idref="c2"/><itemref idref="out"/></spine></package>"#;
        let entries = [
            entry("OEBPS/text/two.xhtml", "2"), entry("cover.jpg", "j"), entry("OEBPS/pkg/style.css", "s"), entry("OEBPS/text/one a.xhtml", "1"),
            entry("OEBPS/pkg/content.opf", opf), entry("META-INF/container.xml", CONTAINER), entry("OEBPS/pkg/toc.ncx", "n"),
            entry("mimetype", "application/epub+zip"), entry("OEBPS/pkg/nav.xhtml", "v"), entry("extra.txt", "x"),
        ];
        let (order, head) = order(&entries);
        assert_eq!(order[..2], ["mimetype", "META-INF/container.xml"]);
        assert_eq!(order[2], "OEBPS/pkg/content.opf");
        // navigation in ZIP order, then style sheets: the head ends where the spine begins
        assert_eq!(order[3..6], ["OEBPS/pkg/toc.ncx", "OEBPS/pkg/nav.xhtml", "OEBPS/pkg/style.css"]);
        assert_eq!(head, 6);
        // the spine in its own order, with the percent-escape undone; then what is left, in ZIP order
        assert_eq!(order[6..8], ["OEBPS/text/one a.xhtml", "OEBPS/text/two.xhtml"]);
        assert_eq!(order[8..], ["cover.jpg", "extra.txt"]);
    }

    #[test]
    fn a_package_document_that_is_not_parsed_leaves_the_zip_order() {
        let deep = format!("<package>{}{}</package>", "<a>".repeat(300), "</a>".repeat(300));
        let entity = r#"<!DOCTYPE package [<!ENTITY e "x">]><package><manifest/><spine/></package>"#;
        for opf in [deep.as_str(), entity, "<package><unclosed></package>", "not xml"] {
            let entries = [entry("z.xhtml", "z"), entry("OEBPS/pkg/content.opf", opf), entry("a.xhtml", "a"), entry("META-INF/container.xml", CONTAINER), entry("mimetype", "m")];
            let (order, head) = order(&entries);
            assert_eq!(order, ["mimetype", "META-INF/container.xml", "z.xhtml", "OEBPS/pkg/content.opf", "a.xhtml"]);
            assert_eq!(head, 2);
        }
    }

    #[test]
    fn nesting_is_never_underestimated() {
        assert_eq!(nesting("<a><b/><c></c></a>"), 2);
        assert_eq!(nesting("<a b='/>'><c d=\"</a>\"></c></a>"), 2);
        assert_eq!(nesting("<!-- <a><a> --><?pi <a> ?><a><![CDATA[<b><b>]]></a>"), 1);
        assert_eq!(nesting(&"<a>".repeat(1000)), 1000);
        assert!(parse_xml(&format!("{}{}", "<a>".repeat(256), "</a>".repeat(256))).is_some());
        assert!(parse_xml(&format!("{}{}", "<a>".repeat(257), "</a>".repeat(257))).is_none());
        assert!(parse_xml("<!DOCTYPE a [<!ENTITY e 'x'>]><a>&e;</a>").is_none());
        assert!(parse_xml("<!DOCTYPE a><a/>").is_some());
    }
}
