//! The input is a list of members to write; whatever the writer accepts must read back identically.
#![no_main]

use ebk::{CodePage, Error, Options, Pack, Reader, Writer};

const NAMES: [&str; 12] = ["mimetype", "a", "a/b", "a/b/c", "b", "OEBPS/第一章.xhtml", "OEBPS/x.css", "img/1.jpg", "a.b", "a-b", "é", "a/"];

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let Some((&[quality, threads, block], mut rest)) = data.split_first_chunk::<3>() else { return };
    let code_page = [CodePage::Auto, CodePage::Never, CodePage::Always][usize::from(quality >> 2) % 3];
    let opts = Options { block_size: (1 << 16) + u64::from(block) * 4096, quality: u32::from(quality % 4), threads: 1 + usize::from(threads % 3), code_page };
    let mut writer = Writer::new(opts).unwrap();
    let mut members: Vec<(String, Vec<u8>)> = Vec::new();
    // per member: name, flags, two bytes of length, one byte that seeds the content
    while let Some((&[name, flags, len_low, len_high, seed], tail)) = rest.split_first_chunk::<5>() {
        rest = tail;
        let mut path = NAMES[usize::from(name) % NAMES.len()].to_owned();
        if name >= 128 {
            path.push_str(&format!("{}", name % 5));
        }
        let len = usize::from(u16::from_le_bytes([len_low, len_high])) * if flags & 8 != 0 { 9 } else { 1 };
        let alphabet = if flags & 4 != 0 { 251 } else { 7 };
        let chinese = flags & 16 != 0;
        let mut state = u32::from(seed).wrapping_mul(2654435761) | 1;
        let mut content: Vec<u8> = (0..len).map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            ((state >> 24) % alphabet) as u8
        }).collect();
        if chinese {
            // text for the code page: a few frequent characters, many rare ones, some outside the BMP
            content = content.iter().map(|&b| char::from_u32(match b % 16 { 0..=9 => 0x4E00 + u32::from(b % 40), 10..=13 => 0x5000 + u32::from(b) * 37, 14 => 0x20000 + u32::from(b), _ => u32::from(b % 0x60) + 0x20 }).unwrap()).collect::<String>().into_bytes();
        }
        let added = match flags & 3 {
            0 => writer.add_text(&path, &content),
            1 => writer.add_resource(&path, content.clone(), Pack::Brotli),
            2 => writer.add_resource(&path, content.clone(), if flags & 32 != 0 { Pack::Jpeg } else { Pack::AsIs }),
            _ => {
                writer.end_head();
                continue;
            }
        };
        match added {
            Ok(()) => members.push((path, content)),
            Err(Error::Invalid(_)) => {}
            Err(e) => panic!("add: unexpected error {e:?}"),
        }
    }
    let mut file = Vec::new();
    match writer.finish(&mut file) {
        Ok(summary) => assert_eq!(summary.file_len, file.len() as u64),
        // two names that cannot be in one file ("a" and "a/b") are only found at the end; nothing else may be refused
        Err(Error::Invalid(why)) => {
            let clash = members.iter().any(|(a, _)| members.iter().any(|(b, _)| b.strip_prefix(a.as_str()).is_some_and(|rest| rest.starts_with('/'))));
            assert!(clash, "finish refused members that can be in one file: {why}");
            return;
        }
        Err(e) => panic!("finish: unexpected error {e:?}"),
    }
    let mut reader = Reader::open(&file[..]).expect("the writer's output does not open");
    assert_eq!(reader.member_count(), members.len());
    assert!(reader.blocks().iter().rev().skip(1).all(|b| b.raw_len >= ebk::MIN_BLOCK_LEN));
    for (i, (path, content)) in members.iter().enumerate() {
        assert_eq!(reader.member(i).unwrap().path, path);
        assert_eq!(&reader.read(i).expect("a member does not read back"), content, "{path:?}");
    }
});
