//! The input is a data area and an uncompressed index; the header, the compression of the index and the footer are
//! made valid here, so that mutations reach the index decoder and the readers of blocks and resource items.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Some((body, index)) = ebk_fuzz::split(data) {
        ebk_fuzz::open_and_read(&ebk_fuzz::wrap(body, index), false);
    }
});
