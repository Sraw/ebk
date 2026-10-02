//! The input is a file handed to the writer as a JPEG to recompress, as the converter does with the pictures of an
//! EPUB. Whatever the writer makes of it - a Lepton file or the bytes as they are - must read back identically,
//! and the codec must not panic on the way.
#![no_main]

use ebk::{Options, Pack, Reader, Writer};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let mut writer = Writer::new(Options { threads: 1, quality: 0, ..Options::default() }).unwrap();
    writer.add_resource("p.jpg", data.to_vec(), Pack::Jpeg).unwrap();
    let mut file = Vec::new();
    writer.finish(&mut file).unwrap();
    let mut reader = Reader::open(&file[..]).expect("the writer's output does not open");
    assert!(reader.read(0).expect("the picture does not read back") == data);
});
