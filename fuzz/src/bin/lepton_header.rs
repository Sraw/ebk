//! Like `lepton`, but the input holds the header of the Lepton file before deflate: 28 bytes of fixed header, the
//! length of the inflated header (2 bytes), that header, then the coded image. The target deflates the header and
//! puts the file together, so that the fuzzer changes the records of the header (the JPEG's own headers, thread
//! handoffs, restart counts, truncation) and not the bits of a deflate stream, which it cannot do usefully.
#![no_main]

use std::io::Write;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if data.len() < 30 {
        return;
    }
    let (fixed, rest) = data.split_at(28);
    let header_len = usize::from(u16::from_le_bytes([rest[0], rest[1]])).min(rest.len() - 2);
    let (header, image) = rest[2..].split_at(header_len);
    let mut deflate = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    deflate.write_all(header).unwrap();
    let packed = deflate.finish().unwrap();

    let mut file = fixed.to_vec();
    file[24..28].copy_from_slice(&(packed.len() as u32).to_le_bytes());
    file.extend_from_slice(&packed);
    file.extend_from_slice(image);
    let total = file.len() as u32 + 4;
    file.extend_from_slice(&total.to_le_bytes());
    ebk_fuzz::decode_lepton_file(&file);
});
