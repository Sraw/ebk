//! Write files with the crate's writer, read them back, then damage them in every way that is cheap to enumerate.

use ebk::{CodePage, Error, Mode, Options, Pack, Reader, Source, Writer};

fn text(seed: u32, len: usize) -> Vec<u8> {
    // compressible but not trivial: words drawn from a small vocabulary
    let words = ["the ", "<p>", "</p>\n", "章", "回", "of ", "and ", "书", "reader ", "，"];
    let mut state = seed.wrapping_mul(2654435761) | 1;
    let mut out = Vec::with_capacity(len + 8);
    while out.len() < len {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        out.extend_from_slice(words[state as usize % words.len()].as_bytes());
    }
    out
}

fn noise(seed: u32, len: usize) -> Vec<u8> {
    let mut state = seed | 1;
    (0..len).map(|_| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state >> 8) as u8
    }).collect()
}

/// (path, bytes, in the text stream)
fn sample() -> Vec<(String, Vec<u8>, bool)> {
    let mut members = vec![
        ("mimetype".to_owned(), b"application/epub+zip".to_vec(), true),
        ("OEBPS/empty.css".to_owned(), Vec::new(), true),
        ("OEBPS/图/cover.jpg".to_owned(), noise(7, 5000), false),
        ("OEBPS/font.ttf".to_owned(), vec![0u8; 40000], false),
        ("OEBPS/empty.bin".to_owned(), Vec::new(), false),
    ];
    members.push(("OEBPS/c0.xhtml.bak".to_owned(), text(99, 100), true)); // sorts between a path and what would be its directory
    for i in 0..12 {
        members.push((format!("OEBPS/c{i}.xhtml"), text(i, 30000 + 9000 * i as usize), true));
    }
    members.push(("OEBPS/last-and-empty.txt".to_owned(), Vec::new(), true)); // sits at the very end of the stream
    members
}

fn build(members: &[(String, Vec<u8>, bool)], block_size: u64) -> Vec<u8> {
    let mut writer = Writer::new(Options { block_size, quality: 5, threads: 2, ..Options::default() }).unwrap();
    for (path, data, in_stream) in members {
        if *in_stream {
            writer.add_text(path, data).unwrap();
        } else {
            writer.add_resource(path, data.clone(), Pack::Brotli).unwrap();
        }
    }
    let mut out = Vec::new();
    let summary = writer.finish(&mut out).unwrap();
    assert_eq!(summary.file_len, out.len() as u64);
    out
}

#[test]
fn members_read_back_identically() {
    let members = sample();
    let file = build(&members, 1 << 16);
    let mut reader = Reader::open(&file[..]).unwrap();
    assert!(reader.blocks().len() > 4, "the sample must span several blocks");
    assert_eq!(reader.member_count(), members.len());
    // backwards, so that the block cache is exercised out of order
    for (path, data, in_stream) in members.iter().rev() {
        let i = reader.find(path).unwrap();
        assert_eq!(reader.member(i).unwrap().mode.in_stream(), *in_stream);
        assert_eq!(&reader.read(i).unwrap(), data, "{path}");
    }
    let mode = |path: &str| reader.member(reader.find(path).unwrap()).unwrap().mode;
    assert_eq!(mode("OEBPS/font.ttf"), Mode::Brotli);
    assert_eq!(mode("OEBPS/图/cover.jpg"), Mode::Raw); // noise does not shrink
    assert_eq!(mode("OEBPS/empty.bin"), Mode::Raw);
}

#[test]
fn empty_book_and_stream_that_fills_its_last_block() {
    let file = build(&[], 1 << 16);
    assert_eq!(Reader::open(&file[..]).unwrap().members().len(), 0);

    let members = vec![("a".to_owned(), text(1, 1 << 17)[..1 << 17].to_vec(), true)];
    let file = build(&members, 1 << 16);
    let mut reader = Reader::open(&file[..]).unwrap();
    assert_eq!(reader.blocks().len(), 2);
    assert_eq!(reader.read(0).unwrap(), members[0].1);
}

#[test]
fn writer_rejects_bad_paths() {
    for path in ["", "/a", "a/", "a//b", "../a", "a/./b", "a\\b", "a\nb"] {
        let mut writer = Writer::new(Options::default()).unwrap();
        assert!(writer.add_text(path, b"x").is_err(), "{path:?}");
    }
    let mut writer = Writer::new(Options::default()).unwrap();
    writer.add_text("a", b"x").unwrap();
    assert!(writer.add_resource("a", vec![1], Pack::AsIs).is_err());
    assert!(Writer::new(Options { block_size: 1000, ..Options::default() }).is_err());

    for (paths, ok) in [(["a", "a.b", "ab"], true), (["a/b", "a.b", "a"], false), (["x/y/z", "x/y", "w"], false), (["x/y/z", "x/yy", "x/y.z"], true)] {
        let mut writer = Writer::new(Options::default()).unwrap();
        for path in paths {
            writer.add_text(path, b"x").unwrap();
        }
        assert_eq!(writer.finish(&mut Vec::new()).is_ok(), ok, "{paths:?}");
    }
}

/// Whatever is done to the file, the reader either refuses it, fails on a member, or returns the original bytes.
fn assert_never_wrong(damaged: &[u8], members: &[(String, Vec<u8>, bool)], what: &str) -> bool {
    let Ok(mut reader) = Reader::open(damaged) else { return true };
    let mut any_error = false;
    for i in 0..reader.member_count() {
        let path = reader.member(i).unwrap().path.to_owned();
        match reader.read(i) {
            Ok(data) => {
                let original = members.iter().find(|m| m.0 == path);
                assert!(original.is_some_and(|m| m.1 == data), "{what}: {path} came back with wrong bytes");
            }
            Err(Error::Corrupt(_) | Error::Unsupported(_) | Error::TooLarge(_) | Error::Io(_)) => any_error = true,
            Err(e) => panic!("{what}: unexpected error kind {e:?}"),
        }
    }
    any_error
}

#[test]
fn truncated_files_are_rejected() {
    let members = sample();
    let file = build(&members, 1 << 16);
    for len in (0..file.len()).step_by(97).chain(file.len() - 40..file.len()) {
        assert!(Reader::open(&file[..len]).is_err(), "a file cut to {len} bytes opened");
    }
    let mut longer = file.clone();
    longer.push(0);
    assert!(Reader::open(&longer[..]).is_err());
}

#[test]
fn flipped_bytes_never_give_wrong_data() {
    let members = sample();
    let file = build(&members, 1 << 16);
    let mut detected = 0;
    let positions = (0..file.len()).step_by(211).chain(0..16).chain(file.len() - 600..file.len());
    for (n, pos) in positions.enumerate() {
        let mut damaged = file.clone();
        damaged[pos] ^= 1 << (n % 8);
        detected += assert_never_wrong(&damaged, &members, &format!("bit flipped at {pos}")) as usize;
    }
    assert!(detected > 100);
    // every byte of the header is checked, the reserved ones included; a later minor version is read
    for pos in 0..16 {
        let mut changed = file.clone();
        changed[pos] ^= 1;
        match Reader::open(&changed[..]) {
            Ok(reader) => assert!(pos == 9 && reader.version() == (1, 1), "header byte {pos}"),
            Err(e) => assert!(pos != 9 && matches!(e, Error::Invalid(_)), "header byte {pos}"),
        }
    }
}

#[test]
fn random_garbage_does_not_panic() {
    let file = build(&sample(), 1 << 16);
    for seed in 1..400u32 {
        // a valid header and footer around noise, and noise in place of the index
        let mut damaged = file.clone();
        let (start, len) = (noise(seed, 8), noise(seed + 1000, 300));
        let at = u64::from_le_bytes(start.try_into().unwrap()) as usize % (file.len() - 300);
        damaged[at..at + 300].copy_from_slice(&len);
        let _ = Reader::open(&damaged[..]).map(|mut r| (0..r.member_count()).map(|i| r.read(i).is_ok()).count());
    }
}

fn brotli_stream(data: &[u8], large_window: bool) -> Vec<u8> {
    let mut packed = Vec::new();
    let mut params = brotli::enc::BrotliEncoderParams { quality: 1, ..Default::default() };
    if large_window {
        params.large_window = true;
        params.lgwin = 30;
    }
    brotli::BrotliCompress(&mut &data[..], &mut packed, &params).unwrap();
    packed
}

/// `body` (header, text blocks and resource items) followed by `raw_index` and a matching footer.
fn with_footer(body: &[u8], raw_index: &[u8], large_window: bool) -> Vec<u8> {
    let packed = brotli_stream(raw_index, large_window);
    let mut out = body.to_vec();
    out.extend_from_slice(&packed);
    out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
    out.extend_from_slice(&(raw_index.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(raw_index).to_le_bytes());
    out.extend_from_slice(b"EBK\x1a");
    out
}

/// Replaces the index of `file` with `raw_index`, with a matching footer, so that damage reaches the index parser.
fn with_index(file: &[u8], raw_index: &[u8]) -> Vec<u8> {
    let old_len = u32::from_le_bytes(file[file.len() - 16..file.len() - 12].try_into().unwrap()) as usize;
    with_footer(&file[..file.len() - 16 - old_len], raw_index, false)
}

fn raw_index(file: &[u8]) -> Vec<u8> {
    let word = |at: usize| u32::from_le_bytes(file[at..at + 4].try_into().unwrap()) as usize;
    let (len, raw_len) = (word(file.len() - 16), word(file.len() - 12));
    let mut out = Vec::new();
    brotli_decompressor::BrotliDecompress(&mut &file[file.len() - 16 - len..file.len() - 16], &mut out).unwrap();
    assert_eq!(out.len(), raw_len);
    out
}

#[test]
fn damaged_index_with_a_valid_checksum_does_not_panic() {
    let members = sample();
    let file = build(&members, 1 << 16);
    let index = raw_index(&file);
    assert!(Reader::open(&with_index(&file, &index)[..]).is_ok(), "the rebuilt file must open before it is damaged");
    let (mut opened, mut rejected) = (0, 0);
    for seed in 1..6000u32 {
        let r = noise(seed, 12);
        let mut damaged = index.clone();
        for change in r.chunks(4).take(1 + seed as usize % 3) {
            let at = u16::from_le_bytes([change[0], change[1]]) as usize % damaged.len();
            match change[2] % 4 {
                0 => damaged[at] = change[3],
                1 => damaged[at] ^= 1 << (change[3] % 8),
                2 => damaged.insert(at, change[3]),
                _ => drop(damaged.remove(at)),
            }
        }
        match Reader::open(&with_index(&file, &damaged)[..]) {
            Ok(mut reader) => {
                opened += 1;
                for i in 0..reader.member_count() {
                    let _ = reader.read(i);
                }
            }
            Err(Error::Invalid(_)) => rejected += 1,
            Err(e) => panic!("unexpected error kind {e:?}"),
        }
    }
    assert!(opened > 50 && rejected > 1000, "opened {opened}, rejected {rejected}");
}

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// A file put together by hand from the specification, so that it can break the rules one at a time.
#[derive(Clone, Default)]
struct Crafted {
    /// (declared raw length, bytes in the file)
    blocks: Vec<(u64, Vec<u8>)>,
    /// (layout, table)
    charset: Option<(u8, Vec<u8>)>,
    /// (mode, path, raw length, stored length, crc32)
    members: Vec<(u8, &'static str, u64, u64, u32)>,
    resources: Vec<u8>,
    optional_section: bool,
}

impl Crafted {
    fn index(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut section = |tag: u8, body: Vec<u8>| {
            out.push(tag);
            varint(&mut out, body.len() as u64);
            out.extend_from_slice(&body);
        };
        let mut body = Vec::new();
        varint(&mut body, self.blocks.len() as u64);
        for (raw_len, data) in &self.blocks {
            varint(&mut body, *raw_len);
            varint(&mut body, data.len() as u64);
        }
        section(1, body);
        if let Some((layout, table)) = &self.charset {
            section(2, [&[*layout][..], table].concat());
        }
        let mut body = Vec::new();
        varint(&mut body, self.members.len() as u64);
        for (mode, path, raw_len, stored_len, crc) in &self.members {
            body.push(*mode);
            varint(&mut body, path.len() as u64);
            body.extend_from_slice(path.as_bytes());
            varint(&mut body, *raw_len);
            varint(&mut body, *stored_len);
            body.extend_from_slice(&crc.to_le_bytes());
        }
        section(3, body);
        if self.optional_section {
            section(0x90, b"something a later version understands".to_vec());
        }
        out
    }

    fn file(&self) -> Vec<u8> {
        let mut body = b"\x89EBK\r\n\x1a\n\x01\x00\0\0\0\0\0\0".to_vec();
        for (_, data) in &self.blocks {
            body.extend_from_slice(data);
        }
        body.extend_from_slice(&self.resources);
        with_footer(&body, &self.index(), false)
    }

    fn open(&self) -> Result<Reader<Vec<u8>>, Error> {
        Reader::open(self.file())
    }
}

/// Two text members in one block, one stored resource and one compressed resource.
fn crafted() -> (Crafted, [Vec<u8>; 4]) {
    let data = [b"<p>first</p>".to_vec(), "<p>第二</p>".as_bytes().to_vec(), noise(3, 50), vec![7u8; 500]];
    let stream = [&data[0][..], &data[1]].concat();
    let packed = brotli_stream(&data[3], false);
    let crc = |d: &[u8]| crc32fast::hash(d);
    let book = Crafted {
        blocks: vec![(stream.len() as u64, brotli_stream(&stream, false))],
        charset: None,
        members: vec![
            (0, "a.xhtml", data[0].len() as u64, data[0].len() as u64, crc(&data[0])),
            (2, "img/a.png", 50, 50, crc(&data[2])),
            (0, "b.xhtml", data[1].len() as u64, data[1].len() as u64, crc(&data[1])),
            (3, "font.ttf", 500, packed.len() as u64, crc(&data[3])),
        ],
        resources: [&data[2][..], &packed].concat(),
        optional_section: false,
    };
    (book, data)
}

fn assert_invalid(book: &Crafted, what: &str) {
    assert!(matches!(book.open(), Err(Error::Invalid(_))), "{what}: {:?}", book.open().err());
}

#[test]
fn a_file_written_from_the_specification_reads_back() {
    let (book, data) = crafted();
    let mut reader = book.open().unwrap();
    for (path, want) in ["a.xhtml", "b.xhtml", "img/a.png", "font.ttf"].into_iter().zip(&data) {
        let i = reader.find(path).unwrap();
        assert_eq!(&reader.read(i).unwrap(), want, "{path}");
    }
    assert_eq!(reader.find("A.xhtml"), None); // lookups are exact

    let mut with_optional = book.clone();
    with_optional.optional_section = true;
    assert_eq!(with_optional.open().unwrap().members().len(), 4);

    reader.set_member_limit(100);
    assert!(matches!(reader.read(3), Err(Error::TooLarge(_))));
    assert!(reader.read(0).is_ok());
    reader.set_member_limit(11); // one byte less than the first text member
    assert!(matches!(reader.read(0), Err(Error::TooLarge(_))));

    assert!(matches!(reader.read(4), Err(Error::NoSuchMember(4))));
    assert!(reader.member(4).is_none());
    // a path that sorts like a member's without being it
    assert_eq!(reader.find("img\0a.png"), None);
    assert_eq!(reader.find("img/a.png"), Some(1));
}

#[test]
fn later_versions_fail_member_by_member() {
    // an unknown storage mode and an unknown code-page layout: the file opens, the members they touch do not
    let (mut book, data) = crafted();
    book.members[1].0 = 9;
    book.members[2].0 = 1;
    book.charset = Some((5, vec![0xFF, 0x00])); // not checked: the layout is unknown
    let mut reader = book.open().unwrap();
    assert!(matches!(reader.read(1), Err(Error::Unsupported(_))));
    assert!(matches!(reader.read(2), Err(Error::Unsupported(_))));
    assert_eq!(reader.read(0).unwrap(), data[0]);
    assert_eq!(reader.read(3).unwrap(), data[3]);

    // bytes that are not a recompressed JPEG (with lengths such a member may have)
    book.members[3].0 = 4;
    book.members[3].2 = 2 * book.members[3].3;
    assert!(matches!(book.open().unwrap().read(3), Err(Error::Corrupt(_))));

    // a table of a known layout over bytes that were never coded with it
    book.charset = Some((1, "二第".as_bytes().to_vec()));
    assert!(matches!(book.open().unwrap().read(2), Err(Error::Corrupt(_))));

    // a later mode may declare any lengths; the members around it still read
    let (mut book, data) = crafted();
    book.members.insert(1, (9, "future", 1 << 62, 0, 0));
    let mut reader = book.open().unwrap();
    assert!(matches!(reader.read(1), Err(Error::Unsupported(_))));
    assert_eq!(reader.read(2).unwrap(), data[2]);
    assert_eq!(reader.read(4).unwrap(), data[3]);
}

#[test]
fn files_that_break_one_rule_are_rejected() {
    let (book, _) = crafted();
    let change = |edit: &dyn Fn(&mut Crafted)| {
        let mut changed = book.clone();
        edit(&mut changed);
        changed
    };

    assert_invalid(&change(&|b| b.charset = Some((1, "二".as_bytes().to_vec()))), "character table without a member that uses it");
    assert_invalid(&change(&|b| b.members[2].0 = 1), "code-page member without a character table");
    for table in [&b"a"[..], "二二".as_bytes(), &[0xE4, 0xBA], &[0xED, 0xA0, 0x80]] {
        assert_invalid(&change(&|b| (b.members[2].0, b.charset) = (1, Some((1, table.to_vec())))), "bad character table");
    }
    assert_invalid(&change(&|b| b.members[3].3 = 500), "compressed resource that is not smaller");
    assert_invalid(&change(&|b| (b.members[3].2, b.members[3].3) = (0, 0)), "empty compressed resource");
    assert_invalid(&change(&|b| b.members[1].3 = 49), "stored resource with two lengths");
    assert_invalid(&change(&|b| b.members[0].2 += 1), "stream member with two lengths");
    assert_invalid(&change(&|b| b.members.push((0, "empty", 0, 0, 1))), "empty member with a checksum");
    assert_invalid(&change(&|b| b.members.push((1, "empty", 0, 0, 0))), "empty member that uses the code page");
    assert_invalid(&change(&|b| b.members[1].1 = "a.xhtml"), "duplicate path");
    assert_invalid(&change(&|b| b.members[1].1 = "a.xhtml/x"), "member under another member");
    assert_invalid(&change(&|b| b.members[0].1 = "img"), "member that is a directory of another");
    for path in ["/a", "a/", "a//b", "..", "a/../b", "./a", "a\\b", "a\u{1}b", "a\u{7f}"] {
        assert_invalid(&change(&|b| b.members[1].1 = path), path);
    }
    assert_invalid(&change(&|b| b.blocks[0].0 += 1), "blocks longer than the stream");
    assert_invalid(&change(&|b| b.blocks.push((1, vec![0x06]))), "a block more than the stream needs");
    assert_invalid(&change(&|b| b.blocks[0].0 = 0), "empty block");
    assert_invalid(&change(&|b| b.resources.push(0)), "a byte nothing describes");
    assert_invalid(&change(&|b| b.members.clear()), "blocks without members");

    // a block may not hide data: its compressed length is bounded by its raw length
    let padded = change(&|b| {
        b.blocks[0].1.extend_from_slice(&[0u8; 80]);
    });
    assert_invalid(&padded, "padded block");
    // within the bound the file opens, and the members of the block fail instead
    let mut reader = change(&|b| b.blocks[0].1.push(0)).open().unwrap();
    assert!(matches!(reader.read(0), Err(Error::Corrupt(_))));
    assert!(reader.read(1).is_ok());

    // the index itself: large-window brotli, a varint with a padding byte, an unknown required section
    let file = book.file();
    let index = book.index();
    assert!(Reader::open(with_index(&file, &index)).is_ok());
    let old_len = u32::from_le_bytes(file[file.len() - 16..file.len() - 12].try_into().unwrap()) as usize;
    assert!(matches!(Reader::open(with_footer(&file[..file.len() - 16 - old_len], &index, true)), Err(Error::Invalid(_))), "large window");
    let mut padded = index.clone();
    assert_eq!(padded[..3], [1, padded[1], 1]); // block table: tag, length, one block
    padded[1] += 1;
    padded[2] = 0x81;
    padded.insert(3, 0x00);
    assert!(matches!(Reader::open(with_index(&file, &padded)), Err(Error::Invalid(_))), "varint with a padding byte");
    let mut unknown = index.clone();
    unknown.extend_from_slice(&[0x7F, 0]);
    assert!(matches!(Reader::open(with_index(&file, &unknown)), Err(Error::Invalid(_))), "unknown required section");
    let mut reordered = vec![0x90, 0];
    reordered.extend_from_slice(&index);
    assert!(matches!(Reader::open(with_index(&file, &reordered)), Err(Error::Invalid(_))), "sections out of order");
}

#[test]
fn incompressible_text_stays_within_the_bound() {
    // 3 MiB of noise in the text stream: the blocks must not grow past what readers accept
    let members = vec![("noise".to_owned(), noise(11, 3 << 20), true)];
    let file = build(&members, 1 << 20);
    let mut reader = Reader::open(&file[..]).unwrap();
    assert_eq!(reader.blocks().len(), 3);
    assert!(reader.blocks().iter().all(|b| b.packed_len >= b.raw_len));
    assert_eq!(reader.read(0).unwrap(), members[0].1);
}

#[test]
fn incompressible_text_at_the_lowest_qualities() {
    // at qualities 0 and 1 the encoder grows noise by more than the bound; the writer must not give up
    for quality in [0, 1] {
        let mut writer = Writer::new(Options { block_size: 1 << 20, quality, threads: 1, ..Options::default() }).unwrap();
        let data = noise(13, 3 << 20);
        writer.add_text("noise", &data).unwrap();
        let mut file = Vec::new();
        writer.finish(&mut file).unwrap();
        assert_eq!(Reader::open(&file[..]).unwrap().read(0).unwrap(), data);
    }
}

#[test]
fn every_block_but_the_last_has_a_minimum_length() {
    let text = text(5, 6000);
    let two_blocks = |first: usize| Crafted {
        blocks: vec![(first as u64, brotli_stream(&text[..first], false)), ((6000 - first) as u64, brotli_stream(&text[first..6000], false))],
        members: vec![(0, "a", 6000, 6000, crc32fast::hash(&text[..6000]))],
        ..Crafted::default()
    };
    assert_eq!(two_blocks(4096).open().unwrap().read(0).unwrap(), &text[..6000]);
    assert_invalid(&two_blocks(4095), "a short block that is not the last");
}

#[test]
fn the_character_table_is_bounded_before_it_is_decoded() {
    let (mut book, _) = crafted();
    book.members[2].0 = 1;
    let distinct = |n: u32| (0..n).map(|i| char::from_u32(0x4E00 + i).unwrap()).collect::<String>().into_bytes();
    book.charset = Some((1, distinct(8128)));
    assert!(book.open().is_ok());
    book.charset = Some((1, distinct(8129)));
    assert_invalid(&book, "8129 characters");
    book.charset = Some((1, "é".repeat(8128 * 2 + 1).into_bytes()));
    assert_invalid(&book, "a table longer than 8128 characters can be");
    // an unknown layout is not looked at, however long
    book.charset = Some((7, vec![b'a'; 1 << 20]));
    assert!(book.open().is_ok());
}

#[test]
fn counts_and_lengths_in_the_index_are_bounded() {
    let (book, _) = crafted();
    let file = book.file();
    let old_len = u32::from_le_bytes(file[file.len() - 16..file.len() - 12].try_into().unwrap()) as usize;
    let body = &file[..file.len() - 16 - old_len];
    let section = |tag: u8, content: &[u8]| {
        let mut out = vec![tag];
        varint(&mut out, content.len() as u64);
        out.extend_from_slice(content);
        out
    };
    let number = |v: u64| {
        let mut out = Vec::new();
        varint(&mut out, v);
        out
    };
    let index = book.index();
    let members_at = index.iter().rposition(|&b| b == 3).unwrap(); // the tag of the member table; no path or length holds a 3
    let (block_table, member_table) = index.split_at(members_at);
    let open = |index: Vec<u8>| Reader::open(with_footer(body, &index, false));
    assert!(open([block_table, member_table].concat()).is_ok());

    // counts larger than the limit, and larger than the bytes that follow could hold
    for count in [(1 << 20) + 1, 1 << 40, 3] {
        let blocks = section(1, &[number(count), vec![0x7F; 4]].concat());
        assert!(matches!(open([&blocks[..], member_table].concat()), Err(Error::Invalid(_))), "{count} blocks");
        let members = section(3, &[number(count), vec![0x7F; 26]].concat());
        assert!(matches!(open([block_table, &members[..]].concat()), Err(Error::Invalid(_))), "{count} members");
    }
    // a nine-byte varint is the longest; ten bytes are refused, as is a length field that runs off the end
    // one member of a later mode in a file with nothing else
    let entry = |raw_len: &[u8]| section(3, &[&[1, 9, 1, b'x'][..], raw_len, &[0, 0, 0, 0, 0]].concat());
    let open_empty = |index: Vec<u8>| Reader::open(with_footer(&file[..16], &index, false));
    let nine = [&[0xFF; 8][..], &[0x7F]].concat();
    let ten = [&[0xFF; 9][..], &[0x01]].concat();
    assert!(matches!(open([block_table, member_table, &[0x90, 0]].concat()), Ok(_)));
    assert!(matches!(open_empty([&section(1, &[0])[..], &entry(&[5])].concat()), Ok(_)));
    assert!(matches!(open_empty([&section(1, &[0])[..], &entry(&nine)].concat()), Ok(_)), "nine-byte varint");
    assert!(matches!(open_empty([&section(1, &[0])[..], &entry(&ten)].concat()), Err(Error::Invalid(_))), "ten-byte varint");
    assert!(matches!(open_empty([&section(1, &[0])[..], &entry(&[0x85, 0])].concat()), Err(Error::Invalid(_))), "varint with a zero byte too many");
    assert!(matches!(open([block_table, &[3, 0x80]].concat()), Err(Error::Invalid(_))), "section length cut off");

    // the footer: an index of no bytes, and one longer than the file
    for index_len in [0u32, file.len() as u32, u32::MAX] {
        let mut damaged = file.clone();
        let at = damaged.len() - 16;
        damaged[at..at + 4].copy_from_slice(&index_len.to_le_bytes());
        assert!(matches!(Reader::open(damaged), Err(Error::Invalid(_))), "index length {index_len}");
    }
    let mut damaged = file.clone();
    let at = damaged.len() - 12;
    damaged[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(Reader::open(damaged), Err(Error::Invalid(_))), "index raw length");
}

#[test]
fn compressed_resources_must_be_exact_brotli_streams() {
    let (book, data) = crafted();
    let packed = brotli_stream(&data[3], false);
    let with = |stored: Vec<u8>, raw_len: u64| {
        let mut changed = book.clone();
        changed.members[3] = (3, "font.ttf", raw_len, stored.len() as u64, crc32fast::hash(&data[3]));
        changed.resources = [&data[2][..], &stored].concat();
        changed
    };
    assert_eq!(with(packed.clone(), 500).open().unwrap().read(3).unwrap(), data[3]);
    let cases = [
        ("a byte after the stream", with([&packed[..], &[0]].concat(), 500)),
        ("a stream longer than declared", with(packed.clone(), 499)),
        ("a stream shorter than declared", with(packed.clone(), 501)),
        ("a truncated stream", with(packed[..packed.len() - 1].to_vec(), 500)),
        ("a large-window stream", with(brotli_stream(&data[3], true), 500)),
    ];
    for (what, book) in cases {
        let mut reader = book.open().unwrap();
        assert!(matches!(reader.read(3), Err(Error::Corrupt(_))), "{what}");
        assert_eq!(reader.read(1).unwrap(), data[2], "{what}: the other resource");
    }
}

/// A file in memory that notes every read.
struct Watched<'a> {
    data: &'a [u8],
    reads: std::cell::RefCell<Vec<(u64, usize)>>,
}

impl Source for Watched<'_> {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.reads.borrow_mut().push((offset, buf.len()));
        self.data.read_at(offset, buf)
    }
}

#[test]
fn a_damaged_block_is_decoded_once() {
    // one block of 200 one-byte members with a byte too many after its brotli stream: the file opens,
    // every member fails, and the block is read from the file a single time
    let stream = text(8, 200)[..200].to_vec();
    let mut block = brotli_stream(&stream, false);
    block.push(0);
    let paths: Vec<&'static str> = (0..200).map(|i| &*format!("m{i}").leak()).collect();
    let book = Crafted {
        blocks: vec![(200, block.clone())],
        members: (0..200).map(|i| (0, paths[i], 1, 1, crc32fast::hash(&stream[i..i + 1]))).collect(),
        ..Crafted::default()
    };
    let file = book.file();
    let mut reader = Reader::open(Watched { data: &file, reads: Default::default() }).unwrap();
    for i in (0..200).chain((0..200).rev()) {
        assert!(matches!(reader.read(i), Err(Error::Corrupt(_))));
    }
    let block_reads = reader.source().reads.borrow().iter().filter(|&&(offset, len)| offset == 16 && len == block.len()).count();
    assert_eq!(block_reads, 1);
}

/// The crafted book with its second text member stored with a code page of two characters.
fn crafted_coded() -> (Crafted, [Vec<u8>; 4]) {
    let (mut book, data) = crafted();
    let coded = [&b"<p>"[..], &[0x81, 0x80], b"</p>"].concat(); // 第 has rank 1, 二 rank 0
    let stream = [&data[0][..], &coded].concat();
    book.blocks = vec![(stream.len() as u64, brotli_stream(&stream, false))];
    book.members[2] = (1, "b.xhtml", data[1].len() as u64, coded.len() as u64, crc32fast::hash(&data[1]));
    book.charset = Some((1, "二第".as_bytes().to_vec()));
    (book, data)
}

#[test]
fn a_coded_member_written_from_the_specification_reads_back() {
    let (book, data) = crafted_coded();
    let mut reader = book.open().unwrap();
    assert_eq!(reader.charset_len(), Some(2));
    assert_eq!(reader.member(2).unwrap().mode, Mode::StreamCoded);
    assert!((0..4).all(|i| reader.readable(i)));
    for (i, original) in [(0, 0), (1, 2), (2, 1), (3, 3)] {
        assert_eq!(reader.read(i).unwrap(), data[original]);
    }

    // an unknown layout: the file opens, the coded member is not readable, the rest is
    let mut later = book.clone();
    later.charset = Some((2, "二第".as_bytes().to_vec()));
    let mut reader = later.open().unwrap();
    assert_eq!(reader.charset_len(), None);
    assert!(!reader.readable(2) && reader.readable(0));
    assert!(matches!(reader.read(2), Err(Error::Unsupported(_))));
    assert_eq!(reader.read(0).unwrap(), data[0]);
}

#[test]
fn a_damaged_code_page_is_an_error_and_never_other_text() {
    let (book, data) = crafted_coded();
    let with_table = |table: &str| {
        let mut changed = book.clone();
        changed.charset = Some((1, table.as_bytes().to_vec()));
        changed
    };
    // tables that decode the member to something else: the same length with other characters, a rank that is
    // not there, characters of another length
    for table in ["第二", "二", "二é", "三四", ""] {
        let mut reader = with_table(table).open().unwrap();
        assert!(matches!(reader.read(2), Err(Error::Corrupt(_))), "table {table:?}");
        assert_eq!(reader.read(0).unwrap(), data[0], "table {table:?}: the member that does not use it");
    }
    // tables that break a rule of the table itself
    assert_invalid(&with_table("a二"), "an ASCII character in the table");
    assert_invalid(&with_table("二第二"), "a character twice");
    let mut bad = book.clone();
    bad.charset = Some((1, vec![0xE4, 0xBA]));
    assert_invalid(&bad, "a table that is not UTF-8");

    // the table and the members that use it come together
    let mut alone = book.clone();
    alone.members[2].0 = 0;
    assert_invalid(&alone, "a table that no member uses");
    let mut missing = book.clone();
    missing.charset = None;
    assert_invalid(&missing, "a coded member without a table");

    // lengths a coded member cannot have: 13 bytes of text from 3, 13 from 27, no text at all
    let coded_len = book.members[2].3;
    for (raw_len, stored_len) in [(13, 3), (13, 27), (0, coded_len), (4 * coded_len + 1, coded_len)] {
        let mut changed = book.clone();
        changed.members[2].2 = raw_len;
        changed.members[2].3 = stored_len;
        assert!(matches!(changed.open(), Err(Error::Invalid(_))), "lengths {raw_len} from {stored_len}");
    }
    // a length within the rules that the data does not have
    for raw_len in [12, 14] {
        let mut changed = book.clone();
        changed.members[2].2 = raw_len;
        assert!(matches!(changed.open().unwrap().read(2), Err(Error::Corrupt(_))), "declared {raw_len} bytes");
    }
}

/// Text in which some characters are frequent and many are rare, like a book's.
fn chinese(seed: u32, chars: usize) -> String {
    let mut state = seed.wrapping_mul(2654435761) | 1;
    (0..chars).map(|_| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let r = state % 1000;
        let code = match r {
            0..=599 => 0x4E00 + (state >> 10) % 50,
            600..=899 => 0x4E00 + (state >> 10) % 2500,
            900..=939 => u32::from(b"<p></p>\n ,."[(state >> 10) as usize % 11]),
            940..=959 => 0x3000 + (state >> 10) % 20,
            _ => 0x20000 + (state >> 10) % 9000,
        };
        char::from_u32(code).unwrap()
    }).collect()
}

fn write_book(members: &[(&str, &[u8])], block_size: u64, code_page: CodePage) -> (Vec<u8>, ebk::Summary) {
    let mut writer = Writer::new(Options { block_size, quality: 5, threads: 2, code_page }).unwrap();
    for (n, (path, data)) in members.iter().enumerate() {
        writer.add_text(path, data).unwrap();
        if n == 1 {
            writer.end_head();
        }
    }
    let mut file = Vec::new();
    let summary = writer.finish(&mut file).unwrap();
    (file, summary)
}

#[test]
fn the_writer_recodes_text_when_that_makes_the_file_smaller() {
    let chapters: Vec<String> = (0..12).map(|n| chinese(n, 30_000)).collect();
    let latin1 = b"caf\xe9 is not UTF-8".to_vec();
    let mut members: Vec<(&str, &[u8])> = vec![("mimetype", b"application/epub+zip"), ("c.opf", "<package>书</package>".as_bytes()), ("empty.css", b""), ("latin1.txt", &latin1)];
    let names: Vec<String> = (0..12).map(|n| format!("第{n}章.xhtml")).collect();
    members.extend(names.iter().zip(&chapters).map(|(name, text)| (name.as_str(), text.as_bytes())));

    let mut lens = Vec::new();
    for (code_page, block_size) in [(CodePage::Never, 1 << 16), (CodePage::Always, 1 << 16), (CodePage::Auto, 1 << 16), (CodePage::Always, 1 << 24)] {
        let (file, summary) = write_book(&members, block_size, code_page);
        let mut reader = Reader::open(&file[..]).unwrap();
        for (i, (path, data)) in members.iter().enumerate() {
            assert_eq!(reader.read(i).unwrap(), *data, "{path} with {code_page:?}");
            // plain ASCII, bytes that are not UTF-8 and empty members stay as they are
            let coded = code_page != CodePage::Never && i != 0 && i != 2 && i != 3;
            assert_eq!(reader.member(i).unwrap().mode, if coded { Mode::StreamCoded } else { Mode::Stream }, "{path} with {code_page:?}");
        }
        assert_eq!(reader.charset_len(), summary.charset_len);
        assert_eq!(summary.charset_len.is_some(), code_page != CodePage::Never);
        // the characters outside the 8128 of the table are spelled out
        assert!(summary.charset_len.is_none_or(|n| n == 8128));
        assert!(block_size == 1 << 24 || reader.blocks().len() > 4);
        assert!(reader.blocks().iter().rev().skip(1).all(|b| b.raw_len >= 4096));
        lens.push(file.len());
    }
    assert!(lens[1] < lens[0] * 95 / 100, "coded {} against plain {}", lens[1], lens[0]);
    assert_eq!(lens[2], lens[1]);
}

#[test]
fn the_writer_keeps_text_as_it_is_when_recoding_does_not_help() {
    // nothing to recode: no table, whatever is asked for
    let ascii: Vec<(&str, &[u8])> = vec![("a", b"plain"), ("b", b"text"), ("c", b"\xff\xfe")];
    for code_page in [CodePage::Auto, CodePage::Always] {
        let (file, summary) = write_book(&ascii, 1 << 16, code_page);
        assert_eq!(summary.charset_len, None);
        assert!(Reader::open(&file[..]).unwrap().members().all(|m| m.mode == Mode::Stream));
    }
    // a little text of rare characters: the table costs more than it saves
    let rare: String = (0..300).map(|i| char::from_u32(0x1F300 + i).unwrap()).collect();
    let members: Vec<(&str, &[u8])> = vec![("a", b"<p>plain</p>"), ("b", rare.as_bytes())];
    let lens: Vec<(usize, Option<usize>)> = [CodePage::Never, CodePage::Always, CodePage::Auto].iter().map(|&code_page| {
        let (file, summary) = write_book(&members, 1 << 16, code_page);
        let mut reader = Reader::open(&file[..]).unwrap();
        assert_eq!(reader.read(1).unwrap(), rare.as_bytes());
        (file.len(), summary.charset_len)
    }).collect();
    assert!(lens[1].0 > lens[0].0 && lens[1].1 == Some(300), "{lens:?}");
    assert_eq!(lens[2], lens[0]);
}

const JPEGS: [(&str, &[u8]); 6] = [
    ("gray.jpg", include_bytes!("data/gray.jpg")),
    ("color.jpg", include_bytes!("data/color.jpg")),
    ("progressive.jpg", include_bytes!("data/progressive.jpg")),
    ("no-eoi.jpg", include_bytes!("data/no-eoi.jpg")),
    ("trailing.jpg", include_bytes!("data/trailing.jpg")),
    ("progressive-no-eoi.jpg", include_bytes!("data/progressive-no-eoi.jpg")),
];

/// A book of the test pictures, and one member that is called a JPEG without being one.
fn picture_book(pack: Pack) -> Vec<u8> {
    let mut writer = Writer::new(Options { threads: 2, quality: 5, ..Options::default() }).unwrap();
    writer.add_text("mimetype", b"application/epub+zip").unwrap();
    for (name, data) in JPEGS {
        writer.add_resource(name, data.to_vec(), pack).unwrap();
    }
    writer.add_resource("not-a.jpg", noise(9, 4000), pack).unwrap();
    let mut file = Vec::new();
    writer.finish(&mut file).unwrap();
    file
}

#[test]
fn jpeg_files_are_recompressed_and_read_back_identically() {
    let (packed, plain) = (picture_book(Pack::Jpeg), picture_book(Pack::AsIs));
    let mut reader = Reader::open(&packed[..]).unwrap();
    let modes: Vec<Mode> = reader.members().map(|m| m.mode).collect();
    // the encoder restores the progressive file without an end marker wrongly and cannot read noise: those stay as
    // they are (whether it takes a progressive file depends on how the file was made; it reads back the same either way)
    assert_eq!([modes[0], modes[1], modes[2], modes[4], modes[5], modes[6], modes[7]], [Mode::Stream, Mode::Jpeg, Mode::Jpeg, Mode::Jpeg, Mode::Jpeg, Mode::Raw, Mode::Raw]);
    for (i, (name, data)) in JPEGS.iter().enumerate() {
        assert_eq!(reader.read(i + 1).unwrap(), *data, "{name}");
        let m = reader.member(i + 1).unwrap();
        // baseline files shrink by a fifth; a progressive one, when the encoder takes it, by less
        assert!(m.mode != Mode::Jpeg || m.stored_len < m.raw_len * if i == 2 { 99 } else { 90 } / 100, "{name}: {} from {}", m.stored_len, m.raw_len);
    }
    assert!(packed.len() < plain.len() * 95 / 100, "{} against {}", packed.len(), plain.len());
    assert!(Reader::open(&plain[..]).unwrap().members().all(|m| m.mode != Mode::Jpeg));

    // an image larger than the reader allows is "too large", and that is not remembered as damage
    reader.set_pixel_limit(320 * 240 - 1);
    assert!(matches!(reader.read(1), Err(Error::TooLarge(_))));
    reader.set_pixel_limit(320 * 240);
    assert_eq!(reader.read(1).unwrap(), JPEGS[0].1);
    reader.set_member_limit(1000);
    assert!(matches!(reader.read(2), Err(Error::TooLarge(_))));
}

#[test]
fn damaged_recompressed_jpegs_are_errors_and_never_other_bytes() {
    let file = picture_book(Pack::Jpeg);
    let reader = Reader::open(&file[..]).unwrap();
    let (gray, color) = (reader.member(1).unwrap(), reader.member(2).unwrap());
    let stream_end = 16 + reader.blocks().iter().map(|b| b.packed_len).sum::<u64>() as usize;
    let (gray_at, color_at) = (stream_end, stream_end + gray.stored_len as usize);
    let (gray_len, color_len) = (gray.stored_len as usize, color.stored_len as usize);

    // every byte of the first 300 of an image flipped in turn, then bytes all over it
    let mut wrong = 0;
    for at in (0..300).chain((300..gray_len).step_by(37)) {
        let mut damaged = file.clone();
        damaged[gray_at + at] ^= 0x10;
        let mut reader = Reader::open(&damaged[..]).unwrap();
        match reader.read(1) {
            Err(Error::Corrupt(_) | Error::TooLarge(_)) => wrong += 1,
            Ok(data) => assert_eq!(data, JPEGS[0].1, "byte {at}: other bytes came back"),
            Err(e) => panic!("byte {at}: unexpected error {e:?}"),
        }
        assert_eq!(reader.read(2).unwrap(), JPEGS[1].1, "byte {at}: the next image");
    }
    assert!(wrong > 250, "only {wrong} damaged files were noticed");

    // the two images in each other's place: both are whole Lepton files, of other pictures
    let mut swapped = file.clone();
    swapped[gray_at..gray_at + color_len].copy_from_slice(&file[color_at..color_at + color_len]);
    swapped[gray_at + color_len..gray_at + color_len + gray_len].copy_from_slice(&file[gray_at..gray_at + gray_len]);
    let mut reader = Reader::open(&swapped[..]).unwrap();
    assert!(matches!(reader.read(1), Err(Error::Corrupt(_))));
    assert!(matches!(reader.read(2), Err(Error::Corrupt(_))));
}

#[test]
fn lengths_of_recompressed_jpegs_are_bounded() {
    let (mut book, data) = crafted();
    // a recompressed JPEG is longer than what is stored for it, at most eight times, and never over 128 MiB
    let stored_len = book.members[3].3;
    book.members[3] = (4, "font.ttf", 8 * stored_len, stored_len, crc32fast::hash(&data[3]));
    assert!(book.open().is_ok());
    book.members[3].2 = 8 * stored_len + 1;
    assert_invalid(&book, "a recompressed JPEG more than eight times what is stored");
    book.members[3].2 = (1 << 27) + 1;
    assert_invalid(&book, "a recompressed JPEG longer than 128 MiB");
    book.members[3].2 = stored_len;
    assert_invalid(&book, "a recompressed JPEG that is not smaller");
}

#[test]
fn stored_lepton_files_decode_to_the_same_jpegs() {
    // files that a reader of storage mode 4 must accept (tests/data/README.md): what an earlier build of the
    // writer stored, so that a change of the codec that alters what these decode to does not go unnoticed
    let files: [(&[u8], &[u8]); 5] = [
        (include_bytes!("data/gray.lep"), include_bytes!("data/gray.jpg")),
        (include_bytes!("data/color.lep"), include_bytes!("data/color.jpg")),
        (include_bytes!("data/progressive.lep"), include_bytes!("data/progressive.jpg")),
        (include_bytes!("data/trailing.lep"), include_bytes!("data/trailing.jpg")),
        (include_bytes!("data/no-eoi.lep"), include_bytes!("data/no-eoi.jpg")),
    ];
    let paths = ["0.jpg", "1.jpg", "2.jpg", "3.jpg", "4.jpg"];
    let book = Crafted {
        members: files.iter().zip(paths).map(|((lepton, jpeg), path)| (4, path, jpeg.len() as u64, lepton.len() as u64, crc32fast::hash(jpeg))).collect(),
        resources: files.iter().flat_map(|(lepton, _)| lepton.iter().copied()).collect(),
        ..Crafted::default()
    };
    let mut reader = book.open().unwrap();
    for (i, (_, jpeg)) in files.iter().enumerate() {
        assert_eq!(reader.read(i).unwrap(), *jpeg, "file {i}");
    }
}
