//! The input is a ZIP file for the converter's own ZIP reader.
#![no_main]

use std::io::Write;

#[allow(dead_code)]
#[path = "../../../crates/ebk-cli/src/epub.rs"]
mod epub;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    // the reader takes a path; /dev/shm keeps the file in memory
    let path = std::path::PathBuf::from(format!("/dev/shm/ebk-fuzz-{}.zip", std::process::id()));
    std::fs::File::create(&path).unwrap().write_all(data).unwrap();
    if let Ok(mut archive) = epub::Archive::open(&path, 16 << 20) {
        let mut entries = Vec::new();
        for i in 0..archive.len() {
            if let Ok(data) = archive.read(i) {
                assert!(data.len() <= 16 << 20, "more came out than the budget allows");
                entries.push(epub::Entry { path: archive.path(i).to_owned(), data, deflated: archive.deflated(i) });
            }
        }
        let (order, head) = epub::reading_order(&entries);
        assert!(head <= order.len() && order.len() == entries.len());
        let mut seen = vec![false; entries.len()];
        for i in order {
            assert!(!std::mem::replace(&mut seen[i], true), "an entry is in the reading order twice");
        }
    }
});
